//! Service Windows « DFWLoggerBigBrother » — exécute Big Brother (protection active)
//! en tant que SYSTEM. Le binaire est le même : `dfwlogger.exe --service`.
//! Installé/désinstallé via l'UI (Mode École → Big Brother) ou `install_big_brother.cmd`,
//! toujours avec élévation consentie par l'admin du parc.

use std::ffi::OsString;
use std::sync::mpsc;
use std::time::Duration;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode,
    ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
use windows_service::{define_windows_service, service_dispatcher};
use windows_service::Result as WsResult;

define_windows_service!(ffi_service_main, service_main);

/// Point d'entrée du dispatcher (appelé depuis main quand args contient --service).
pub fn run_dispatcher() -> Result<(), String> {
    service_dispatcher::start(crate::big_brother::SERVICE_NAME, ffi_service_main)
        .map_err(|e| format!("service : {e}"))
}

fn service_main(args: Vec<OsString>) {
    if let Err(e) = run(args) {
        crate::big_brother::log(&format!("service terminé en erreur : {e}"));
    }
}

fn run(_args: Vec<OsString>) -> WsResult<()> {
    let (tx, rx) = mpsc::channel();

    let handler = service_control_handler::register(
        crate::big_brother::SERVICE_NAME,
        move |event| match event {
            ServiceControl::Stop | ServiceControl::Shutdown => {
                let _ = tx.send(());
                ServiceControlHandlerResult::NoError
            }
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            _ => ServiceControlHandlerResult::NotImplemented,
        },
    )?;

    let status = |state: ServiceState, accept: ServiceControlAccept| ServiceStatus {
        service_type: ServiceType::OWN_PROCESS,
        current_state: state,
        controls_accepted: accept,
        exit_code: ServiceExitCode::Win32(0),
        checkpoint: 0,
        wait_hint: Duration::from_secs(20),
        process_id: None,
    };

    handler.set_service_status(status(ServiceState::StartPending, ServiceControlAccept::empty()))?;

    // Thread de travail : boucle Big Brother. Un seul tick / 15 s.
    std::thread::Builder::new()
        .name("dfwlogger-bigbrother-loop".into())
        .spawn(|| loop {
            crate::big_brother::tick();
            std::thread::sleep(Duration::from_secs(15));
        })
        .ok();

    handler.set_service_status(status(
        ServiceState::Running,
        ServiceControlAccept::STOP | ServiceControlAccept::SHUTDOWN,
    ))?;

    // Reste en vie jusqu'à l'arrêt (la boucle de garde vit dans son thread).
    let _ = rx.recv();

    handler.set_service_status(status(ServiceState::StopPending, ServiceControlAccept::empty()))?;
    handler.set_service_status(status(ServiceState::Stopped, ServiceControlAccept::empty()))?;
    Ok(())
}
