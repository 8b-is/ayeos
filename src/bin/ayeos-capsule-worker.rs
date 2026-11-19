//! Hardened, fail-closed Linux sandbox worker for AyeOS pure-function capsules (`ayeos-capsule-worker`).
//!
//! Host Linux test harness ONLY — never a boot or bare-metal dependency.
//!
//! Security invariants enforced BEFORE `dlopen()` (so ELF `.init_array` constructors cannot escape):
//! 1. Closes all non-IPC file descriptors (`fd >= 2`); worker retains ONLY FD 0 (IPC in) & FD 1 (IPC out).
//! 2. Applies strict `RLIMIT_CORE=0`, `RLIMIT_AS`, `RLIMIT_CPU`, `RLIMIT_NOFILE`.
//! 3. Sets `PR_SET_NO_NEW_PRIVS` (fail-closed).
//! 4. Installs a real Linux **Landlock** ruleset granting read+execute ONLY on the single verified
//!    zero-`DT_NEEDED` `.so` path (fail-closed if Landlock cannot be established).
//! 5. Installs **Stage-1 `seccomp-bpf`** BEFORE `dlopen()` blocking all network, exec, fork/clone,
//!    ptrace, and write-mode `openat` syscalls (`SECCOMP_RET_KILL_PROCESS` on violation; fail-closed).
//! 6. Calls `dlopen(RTLD_NOW | RTLD_LOCAL)` and resolves `ayeos_capsule_abi_version` + `ayeos_capsule_eval`.
//! 7. Stacks **Stage-2 `seccomp-bpf`** AFTER `dlsym()` allowing ONLY `read`, `write`, `clock_gettime`,
//!    `rt_sigreturn`, and `exit_group`/`exit` (fail-closed).
//! 8. Serves fixed 16-byte binary frames on FD 0 -> FD 1 until EOF or parent wall-clock kill.

#![deny(clippy::float_arithmetic)]

use std::env;
use std::ffi::CString;
use std::mem;
use std::os::raw::{c_char, c_int, c_void};
use std::ptr;

const WORKER_READY_MAGIC: [u8; 16] = *b"AYEWRK01READY!\0\0";

#[cfg(target_os = "linux")]
mod linux_sandbox {
    use super::*;

    // Linux Landlock syscall numbers (same on x86_64 and aarch64)
    const SYS_LANDLOCK_CREATE_RULESET: libc::c_long = 444;
    const SYS_LANDLOCK_ADD_RULE: libc::c_long = 445;
    const SYS_LANDLOCK_RESTRICT_SELF: libc::c_long = 446;

    const LANDLOCK_RULE_PATH_BENEATH: u32 = 1;
    // ABI v1 full filesystem access mask (bits 0..=12)
    const LANDLOCK_FS_ALL_V1: u64 = 0x1FFF;
    const LANDLOCK_ACCESS_FS_EXECUTE: u64 = 1 << 0;
    const LANDLOCK_ACCESS_FS_READ_FILE: u64 = 1 << 2;

    #[repr(C)]
    struct LandlockRulesetAttr {
        handled_access_fs: u64,
    }

    #[repr(C, packed)]
    struct LandlockPathBeneathAttr {
        allowed_access: u64,
        parent_fd: i32,
    }

    // Seccomp-BPF definitions
    const SECCOMP_MODE_FILTER: libc::c_ulong = 2;
    const SECCOMP_RET_KILL_PROCESS: u32 = 0x8000_0000;
    const SECCOMP_RET_ALLOW: u32 = 0x7FFF_0000;

    const BPF_LD: u16 = 0x00;
    const BPF_W: u16 = 0x00;
    const BPF_ABS: u16 = 0x20;
    const BPF_JMP: u16 = 0x05;
    const BPF_JEQ: u16 = 0x10;
    const BPF_JSET: u16 = 0x40;
    const BPF_K: u16 = 0x00;
    const BPF_RET: u16 = 0x06;

    const OFFSET_NR: u32 = 0;
    const OFFSET_ARCH: u32 = 4;
    // offsetof(struct seccomp_data, args[2]) = 16 + 2*8 = 32 (low 32 bits on little-endian)
    const OFFSET_ARG2_LO: u32 = 32;

    #[cfg(target_arch = "x86_64")]
    const AUDIT_ARCH: u32 = 0xC000_003E;
    #[cfg(target_arch = "aarch64")]
    const AUDIT_ARCH: u32 = 0xC000_00B7;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct SockFilter {
        code: u16,
        jt: u8,
        jf: u8,
        k: u32,
    }

    #[repr(C)]
    struct SockFprog {
        len: u16,
        filter: *const SockFilter,
    }

    #[inline(always)]
    const fn bpf_stmt(code: u16, k: u32) -> SockFilter {
        SockFilter {
            code,
            jt: 0,
            jf: 0,
            k,
        }
    }

    #[inline(always)]
    const fn bpf_jump(code: u16, k: u32, jt: u8, jf: u8) -> SockFilter {
        SockFilter { code, jt, jf, k }
    }

    pub fn close_non_ipc_fds() -> Result<(), &'static str> {
        // Close everything except FD 0 (IPC read) and FD 1 (IPC write).
        // SYS_close_range = 436 on Linux x86_64 & aarch64.
        let rc = unsafe { libc::syscall(436, 2usize, u32::MAX as usize, 0usize) };
        if rc == 0 {
            return Ok(());
        }
        // Fallback loop if close_range unavailable
        for fd in 2..1024 {
            unsafe {
                let _ = libc::close(fd);
            }
        }
        Ok(())
    }

    pub fn apply_rlimits() -> Result<(), &'static str> {
        unsafe {
            let core0 = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            if libc::setrlimit(libc::RLIMIT_CORE, &core0) != 0 {
                return Err("setrlimit(RLIMIT_CORE) failed");
            }
            let as_lim = libc::rlimit {
                rlim_cur: 256 * 1024 * 1024,
                rlim_max: 256 * 1024 * 1024,
            };
            if libc::setrlimit(libc::RLIMIT_AS, &as_lim) != 0 {
                return Err("setrlimit(RLIMIT_AS) failed");
            }
            let cpu_lim = libc::rlimit {
                rlim_cur: 30,
                rlim_max: 30,
            };
            if libc::setrlimit(libc::RLIMIT_CPU, &cpu_lim) != 0 {
                return Err("setrlimit(RLIMIT_CPU) failed");
            }
            let nofile_lim = libc::rlimit {
                rlim_cur: 16,
                rlim_max: 16,
            };
            if libc::setrlimit(libc::RLIMIT_NOFILE, &nofile_lim) != 0 {
                return Err("setrlimit(RLIMIT_NOFILE) failed");
            }
        }
        Ok(())
    }

    pub fn set_no_new_privs() -> Result<(), &'static str> {
        let rc = unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) };
        if rc != 0 {
            return Err("prctl(PR_SET_NO_NEW_PRIVS) failed");
        }
        Ok(())
    }

    /// Enforce Landlock allowing READ+EXECUTE strictly on the single verified `.so` file.
    /// Fails closed if the kernel refuses Landlock.
    pub fn enforce_landlock_single_so(so_cstr: &CString) -> Result<(), &'static str> {
        let attr = LandlockRulesetAttr {
            handled_access_fs: LANDLOCK_FS_ALL_V1,
        };
        let ruleset_fd = unsafe {
            libc::syscall(
                SYS_LANDLOCK_CREATE_RULESET,
                &attr as *const LandlockRulesetAttr,
                mem::size_of::<LandlockRulesetAttr>(),
                0usize,
            )
        } as c_int;
        if ruleset_fd < 0 {
            return Err("landlock_create_ruleset failed");
        }

        let so_fd = unsafe {
            libc::open(
                so_cstr.as_ptr(),
                libc::O_PATH | libc::O_CLOEXEC,
            )
        };
        if so_fd < 0 {
            unsafe { libc::close(ruleset_fd) };
            return Err("open(O_PATH) on capsule .so failed");
        }

        let path_attr = LandlockPathBeneathAttr {
            allowed_access: LANDLOCK_ACCESS_FS_READ_FILE | LANDLOCK_ACCESS_FS_EXECUTE,
            parent_fd: so_fd,
        };
        let add_rc = unsafe {
            libc::syscall(
                SYS_LANDLOCK_ADD_RULE,
                ruleset_fd,
                LANDLOCK_RULE_PATH_BENEATH,
                &path_attr as *const LandlockPathBeneathAttr,
                0usize,
            )
        };
        unsafe { libc::close(so_fd) };
        if add_rc != 0 {
            unsafe { libc::close(ruleset_fd) };
            return Err("landlock_add_rule failed");
        }

        let restrict_rc = unsafe { libc::syscall(SYS_LANDLOCK_RESTRICT_SELF, ruleset_fd, 0usize) };
        unsafe { libc::close(ruleset_fd) };
        if restrict_rc != 0 {
            return Err("landlock_restrict_self failed");
        }
        Ok(())
    }

    fn install_seccomp_filter(filters: &[SockFilter]) -> Result<(), &'static str> {
        let prog = SockFprog {
            len: filters.len() as u16,
            filter: filters.as_ptr(),
        };
        let rc = unsafe {
            libc::prctl(
                libc::PR_SET_SECCOMP,
                SECCOMP_MODE_FILTER,
                &prog as *const SockFprog,
                0,
                0,
            )
        };
        if rc != 0 {
            return Err("prctl(PR_SET_SECCOMP) failed");
        }
        Ok(())
    }

    /// Stage-1 seccomp-bpf installed BEFORE `dlopen()` so `.init_array` constructors run sandboxed.
    /// Blocks all networking, exec/fork/clone, ptrace, filesystem mutation, and write-mode `openat`.
    #[cfg(target_arch = "x86_64")]
    pub fn install_seccomp_stage1_pre_dlopen() -> Result<(), &'static str> {
        // Allowed syscalls strictly for glibc dlopen() of a zero-DT_NEEDED .so + IPC + Stage-2 prctl
        const ALLOWED: &[u32] = &[
            0,   // read
            1,   // write
            3,   // close
            5,   // fstat
            9,   // mmap
            10,  // mprotect
            11,  // munmap
            12,  // brk
            15,  // rt_sigreturn
            17,  // pread64
            60,  // exit
            157, // prctl (needed to stack Stage-2 seccomp-bpf; NO_NEW_PRIVS already set)
            202, // futex
            228, // clock_gettime
            231, // exit_group
            262, // newfstatat
            318, // getrandom
            332, // statx
            334, // rseq
        ];
        // openat (257) is handled specially below to forbid any write/create flags.
        const NR_OPENAT: u32 = 257;
        const WRITE_OPEN_FLAGS: u32 =
            (libc::O_WRONLY | libc::O_RDWR | libc::O_CREAT | libc::O_TRUNC | libc::O_APPEND) as u32;

        let mut f: Vec<SockFilter> = Vec::with_capacity(ALLOWED.len() + 12);
        // 1. Verify architecture == x86_64
        f.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, OFFSET_ARCH));
        f.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH, 1, 0));
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));

        // 2. Load syscall number
        f.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, OFFSET_NR));

        // 3. Special check for openat(dirfd, path, flags, mode): allow ONLY if no write flags set
        // Jump offsets: if eq NR_OPENAT, check flags (3 insns); else skip past openat block
        f.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, NR_OPENAT, 0, 4));
        f.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, OFFSET_ARG2_LO));
        f.push(bpf_jump(BPF_JMP | BPF_JSET | BPF_K, WRITE_OPEN_FLAGS, 0, 1));
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));

        // 4. Exact allowlist
        // Note: if not openat, accumulator still holds syscall NR.
        let allow_idx = f.len() + ALLOWED.len() + 1;
        for (i, &nr) in ALLOWED.iter().enumerate() {
            let cur_idx = f.len();
            let jt = (allow_idx - cur_idx - 1) as u8;
            let _ = i;
            f.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, nr, jt, 0));
        }
        // Default: KILL_PROCESS
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));
        // Allow target
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));

        install_seccomp_filter(&f)
    }

    /// Stage-2 seccomp-bpf stacked AFTER `dlsym()`: drops `openat`, `mmap`, `mprotect`, `prctl`, etc.,
    /// leaving ONLY pure compute + IPC (`read`, `write`, `clock_gettime`, `rt_sigreturn`, `exit_group`).
    #[cfg(target_arch = "x86_64")]
    pub fn install_seccomp_stage2_post_dlopen() -> Result<(), &'static str> {
        const ALLOWED_PURE: &[u32] = &[
            0,   // read
            1,   // write
            15,  // rt_sigreturn
            60,  // exit
            228, // clock_gettime
            231, // exit_group
        ];

        let mut f: Vec<SockFilter> = Vec::with_capacity(ALLOWED_PURE.len() + 6);
        f.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, OFFSET_ARCH));
        f.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, AUDIT_ARCH, 1, 0));
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));

        f.push(bpf_stmt(BPF_LD | BPF_W | BPF_ABS, OFFSET_NR));
        let allow_idx = f.len() + ALLOWED_PURE.len() + 1;
        for &nr in ALLOWED_PURE {
            let cur_idx = f.len();
            let jt = (allow_idx - cur_idx - 1) as u8;
            f.push(bpf_jump(BPF_JMP | BPF_JEQ | BPF_K, nr, jt, 0));
        }
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_KILL_PROCESS));
        f.push(bpf_stmt(BPF_RET | BPF_K, SECCOMP_RET_ALLOW));

        install_seccomp_filter(&f)
    }
}

fn read_exact_fd(fd: c_int, buf: &mut [u8]) -> bool {
    let mut offset = 0usize;
    while offset < buf.len() {
        let n = unsafe {
            libc::read(
                fd,
                buf.as_mut_ptr().add(offset) as *mut c_void,
                buf.len() - offset,
            )
        };
        if n <= 0 {
            return false;
        }
        offset += n as usize;
    }
    true
}

fn write_all_fd(fd: c_int, buf: &[u8]) -> bool {
    let mut offset = 0usize;
    while offset < buf.len() {
        let n = unsafe {
            libc::write(
                fd,
                buf.as_ptr().add(offset) as *const c_void,
                buf.len() - offset,
            )
        };
        if n <= 0 {
            return false;
        }
        offset += n as usize;
    }
    true
}

fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    (ts.tv_sec as u64)
        .saturating_mul(1_000_000_000)
        .saturating_add(ts.tv_nsec as u64)
}

fn fail_closed(code: i32) -> ! {
    unsafe {
        libc::_exit(code);
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        fail_closed(100);
    }
    let Ok(so_cstr) = CString::new(args[1].as_str()) else {
        fail_closed(101);
    };

    #[cfg(not(all(target_os = "linux", target_arch = "x86_64")))]
    {
        // Fail closed on platforms where this Linux sandbox cannot be established.
        fail_closed(102);
    }

    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        // --- ALL ISOLATION MUST BE ACTIVE *BEFORE* dlopen() ---
        if linux_sandbox::close_non_ipc_fds().is_err() {
            fail_closed(103);
        }
        if linux_sandbox::apply_rlimits().is_err() {
            fail_closed(104);
        }
        if linux_sandbox::set_no_new_privs().is_err() {
            fail_closed(105);
        }
        if linux_sandbox::enforce_landlock_single_so(&so_cstr).is_err() {
            fail_closed(106);
        }
        if linux_sandbox::install_seccomp_stage1_pre_dlopen().is_err() {
            fail_closed(107);
        }

        // Now load the verified zero-DT_NEEDED .so inside the active sandbox.
        // Any hostile .init_array constructor runs inside Landlock + Stage-1 seccomp-bpf.
        let handle = unsafe { libc::dlopen(so_cstr.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            fail_closed(108);
        }

        let abi_sym = b"ayeos_capsule_abi_version\0";
        let eval_sym = b"ayeos_capsule_eval\0";
        let abi_ptr = unsafe { libc::dlsym(handle, abi_sym.as_ptr() as *const c_char) };
        let eval_ptr = unsafe { libc::dlsym(handle, eval_sym.as_ptr() as *const c_char) };
        if abi_ptr.is_null() || eval_ptr.is_null() {
            fail_closed(109);
        }

        // Tighten to Stage-2 pure-compute + IPC-only seccomp BEFORE calling any capsule symbol!
        if linux_sandbox::install_seccomp_stage2_post_dlopen().is_err() {
            fail_closed(110);
        }

        let abi_fn: extern "C" fn() -> u32 = unsafe { mem::transmute(abi_ptr) };
        let eval_fn: extern "C" fn(i64, i64) -> i64 = unsafe { mem::transmute(eval_ptr) };

        if abi_fn() != 1 {
            fail_closed(111);
        }

        // Signal READY on IPC stdout (FD 1)
        if !write_all_fd(1, &WORKER_READY_MAGIC) {
            fail_closed(112);
        }

        // Pure-integer IPC request/response loop on FD 0 -> FD 1
        let mut req = [0u8; 16];
        let mut resp = [0u8; 16];
        loop {
            if !read_exact_fd(0, &mut req) {
                break;
            }
            let seed = i64::from_le_bytes([
                req[0], req[1], req[2], req[3], req[4], req[5], req[6], req[7],
            ]);
            let rounds = i64::from_le_bytes([
                req[8], req[9], req[10], req[11], req[12], req[13], req[14], req[15],
            ]);

            let t0 = monotonic_ns();
            let result = eval_fn(seed, rounds);
            let t1 = monotonic_ns();
            let elapsed_ns = t1.saturating_sub(t0);

            resp[0..8].copy_from_slice(&result.to_le_bytes());
            resp[8..16].copy_from_slice(&elapsed_ns.to_le_bytes());
            if !write_all_fd(1, &resp) {
                break;
            }
        }

        let _ = ptr::null::<u8>();
        unsafe { libc::_exit(0) };
    }
}
