//! Управление жизненным циклом подпроцесса `lumen-renderer` (PH3-GPUSANDBOX
//! Phase A, срез A3 — шаг 3 из `docs/tasks/ph3-gpu-process-sandbox.md`;
//! срез A4 добавляет шаг 8 — Windows Job Object).
//!
//! Зеркалит `network_service.rs`: `spawn()` запускает бинарник, читает порт из
//! его первой строки stdout, подключается и возвращает готовый `IpcClient`.
//! Дроп хендла убивает дочерний процесс.
//!
//! # Job Object (Windows, срез A4)
//!
//! `Drop::drop` вызывает `child.kill()`, но дроп не гарантирован: `TerminateProcess`
//! на самом шелле (крэш, Task Manager "End task", `kill -9` на дев-машине) обходит
//! деструкторы, и `lumen-renderer.exe` остаётся сиротой. На Windows дочерний процесс
//! назначается анонимному Job Object с `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` — ОС сама
//! убивает все процессы job-а, когда последний хендл на него закрывается (в т.ч. когда
//! ядро закрывает хендлы шелла при его аварийном завершении), независимо от того,
//! добежал ли `Drop` до `child.kill()`.
//!
//! # Статус подключения
//!
//! Ничего в шелле пока не вызывает `RendererProcessHandle::spawn()` — шелл
//! по-прежнему рисует через `backend_factory::create_backend()` (in-process
//! путь). `lumen-renderer` (срез A2) отвечает на `GpuInit`/`GpuRender` без
//! реального `wgpu::Device`, так что подмена живого бэкенда на этот процесс
//! сейчас дала бы пустой экран. Замена (`RemoteRenderBackend`, шаг 5) —
//! отдельный срез, после того как рендерер получит настоящий wgpu-путь.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use lumen_core::error::{Error, Result};
use lumen_ipc::IpcClient;

/// Анонимный Job Object с `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, живущий ровно
/// столько же, сколько `RendererProcessHandle` (см. модульную документацию
/// §Job Object). Не Windows — не существует, платформенное ограничение шага 8.
#[cfg(windows)]
struct KillOnCloseJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl KillOnCloseJob {
    /// Создаёт анонимный job, включает `KILL_ON_JOB_CLOSE` и назначает в него
    /// `process`. При любой ошибке созданный (если успел) хендл закрывается —
    /// вызывающая сторона получает `Err` и падает обратно на "только `child.kill()`
    /// в `Drop`" вместо паники (сам процесс `lumen-renderer` уже запущен и рисует).
    fn new(process: windows_sys::Win32::Foundation::HANDLE) -> Result<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        // SAFETY: null attributes/name creates an anonymous, unnamed job object
        // with default security; the call has no other preconditions. Failure
        // is signalled by a null return, checked immediately below.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job == 0 {
            return Err(Error::Io(format!(
                "CreateJobObjectW failed: {}",
                std::io::Error::last_os_error()
            )));
        }

        // SAFETY: zero-init is valid for this struct — every field is a plain
        // integer, and only `BasicLimitInformation.LimitFlags` is set below;
        // the rest (memory/CPU/affinity limits) staying zero means "no limit",
        // matching the goal (kill-on-close only, no other constraint on the
        // renderer).
        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        // SAFETY: `job` was just created above and is a valid handle; `info`
        // is a live local of exactly the type/size `SetInformationJobObject`
        // expects for `JobObjectExtendedLimitInformation`.
        let set_ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if set_ok == 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: `job` is the valid handle created above; closing it here
            // (the constructor is failing) matches the "no leak on Err" contract.
            unsafe { CloseHandle(job) };
            return Err(Error::Io(format!("SetInformationJobObject failed: {err}")));
        }

        // SAFETY: `job` and `process` are both valid, open handles for the
        // duration of this call (the caller owns `process` via `Child`).
        let assign_ok = unsafe { AssignProcessToJobObject(job, process) };
        if assign_ok == 0 {
            let err = std::io::Error::last_os_error();
            // SAFETY: same as above — `job` is valid and we are on the Err path.
            unsafe { CloseHandle(job) };
            return Err(Error::Io(format!("AssignProcessToJobObject failed: {err}")));
        }

        Ok(Self(job))
    }
}

#[cfg(windows)]
impl Drop for KillOnCloseJob {
    fn drop(&mut self) {
        // SAFETY: `self.0` was created by `CreateJobObjectW` in `new` and is
        // not closed anywhere else — this is the single, sole close site.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Хендл живого подпроцесса `lumen-renderer`.
///
/// Дроп хендла убивает дочерний процесс (и тем самым закрывает TCP-соединение).
/// На Windows дочерний процесс также состоит в kill-on-close Job Object (см.
/// модульную документацию §Job Object) — переживает крэш/`kill` самого шелла,
/// не только его обычный `Drop`.
pub struct RendererProcessHandle {
    child: Child,
    #[cfg(windows)]
    job: KillOnCloseJob,
}

impl RendererProcessHandle {
    /// Запустить `lumen-renderer` из той же директории, что и текущий исполняемый файл.
    ///
    /// Блокирует до тех пор, пока процесс не напечатает порт и шелл не подключится.
    ///
    /// Возвращает `(handle, client)`:
    /// - `handle` — хендл процесса; держи живым, пока нужен рендерер
    /// - `client` — IPC-клиент для отправки `GpuInit`/`GpuRender`/`GpuResize`/`GpuSurfaceLost`
    pub fn spawn() -> Result<(Self, IpcClient)> {
        let exe_path = std::env::current_exe()
            .map_err(|e| Error::Io(format!("current_exe: {e}")))?;
        let exe_dir = exe_path
            .parent()
            .ok_or_else(|| Error::Io("current_exe has no parent dir".into()))?;

        // On Windows the binary has .exe extension; on Unix there's none.
        #[cfg(windows)]
        let bin_name = "lumen-renderer.exe";
        #[cfg(not(windows))]
        let bin_name = "lumen-renderer";

        let bin_path = exe_dir.join(bin_name);

        let mut child = Command::new(&bin_path)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| Error::Io(format!("spawn {}: {e}", bin_path.display())))?;

        // Assign the freshly spawned process to a kill-on-close Job Object
        // (see module docs §Job Object) before anything else can fail and
        // orphan it — an early `?` return below now kills the child too,
        // since `job`'s `Drop` closes the last handle on the job.
        #[cfg(windows)]
        let job = {
            use std::os::windows::io::AsRawHandle;
            KillOnCloseJob::new(child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE)?
        };

        // Read the port number printed by lumen-renderer on its first stdout line.
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| Error::Io("no stdout from renderer process".into()))?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|e| Error::Io(format!("read port from renderer process: {e}")))?;

        let port: u16 = line.trim().parse().map_err(|e| {
            Error::Io(format!(
                "renderer process printed invalid port {:?}: {e}",
                line.trim()
            ))
        })?;

        let client = IpcClient::connect(port)?;
        Ok((
            Self {
                child,
                #[cfg(windows)]
                job,
            },
            client,
        ))
    }
}

impl Drop for RendererProcessHandle {
    fn drop(&mut self) {
        // Kill the child; it exits cleanly when its TCP socket is closed anyway.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests {
    /// Smoke test: verify RendererProcessHandle::spawn() fails gracefully when
    /// the binary is absent (expected in test environment — no actual binary
    /// built as part of `cargo test -p lumen-shell`).
    #[test]
    fn spawn_missing_binary_returns_error() {
        let result = super::RendererProcessHandle::spawn();
        assert!(result.is_err(), "expected Err when binary is absent");
    }

    /// Срез A4: closing the last handle on a `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`
    /// job must kill every process assigned to it — this is the exact mechanism
    /// `RendererProcessHandle` relies on to not leave `lumen-renderer.exe` alive
    /// after the shell itself is `TerminateProcess`-ed (see module docs
    /// §Job Object). Spawns an unrelated long-lived `cmd.exe` (not the renderer
    /// binary, which is absent in the test environment) as a stand-in child.
    #[cfg(windows)]
    #[test]
    fn kill_on_close_job_kills_assigned_process_when_dropped() {
        use std::os::windows::io::AsRawHandle;

        let mut child = std::process::Command::new("cmd")
            .args(["/C", "timeout", "/T", "30"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .expect("spawn stand-in child process");

        let job = super::KillOnCloseJob::new(child.as_raw_handle() as windows_sys::Win32::Foundation::HANDLE)
            .expect("create kill-on-close job");
        assert!(
            child.try_wait().expect("try_wait before drop").is_none(),
            "child must still be running right after being assigned to the job"
        );

        drop(job); // closes the job's only handle -> OS kills every assigned process

        // Give the kernel a moment to deliver the termination; poll instead of
        // asserting instantly, since `AssignProcessToJobObject`'s kill-on-close
        // is asynchronous relative to `CloseHandle` returning.
        let mut killed = false;
        for _ in 0..50 {
            if child.try_wait().expect("try_wait after drop").is_some() {
                killed = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(killed, "process must be killed once the job's last handle closes");

        let _ = child.kill();
        let _ = child.wait();
    }
}
