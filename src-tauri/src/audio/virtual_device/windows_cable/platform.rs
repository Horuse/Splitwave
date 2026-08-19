use std::collections::HashMap;
use std::ffi::{c_void, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    SetupDiDestroyDeviceInfoList, SetupDiEnumDeviceInfo, SetupDiGetClassDevsW,
    SetupDiGetDeviceInstanceIdW, SetupDiGetDevicePropertyW, DIGCF_ALLCLASSES, SP_DEVINFO_DATA,
};
use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::Devices::Properties::{
    DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_DriverInfPath, DEVPKEY_Device_DriverProvider,
    DEVPKEY_Device_DriverVersion, DEVPKEY_Device_FriendlyName, DEVPKEY_Device_Parent,
};
use windows::Win32::Foundation::{
    CloseHandle, LocalFree, ERROR_CANCELLED, ERROR_NO_MORE_ITEMS, HANDLE, HLOCAL,
};
use windows::Win32::Media::Audio::{
    eCapture, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
};
use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
use windows::Win32::Security::{
    GetTokenInformation, TokenElevation, TokenUser, TOKEN_ELEVATION, TOKEN_QUERY, TOKEN_USER,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject, INFINITE,
};
use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY, KEY_WRITE};
use winreg::RegKey;
use zip::ZipArchive;

use super::{
    detect_cable_from_inventory, determine_removal_state, manifest_ownership, removal_decision,
    status_from, verified_package_after_setup, CableEndpoint, CableEndpointFlow, CablePackage,
    DetectedCable, ManifestState, OwnershipManifest, RemovalAction, UninstallReason,
    WindowsVirtualCableError, WindowsVirtualCableOwnership, WindowsVirtualCableStatus,
    MANIFEST_SCHEMA_VERSION, PROVIDER_NAME,
};

const VBCABLE_URL: &str = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip";
const VBCABLE_ARCHIVE_SHA256: &str =
    "B950E39F01AF1D04EA623C8F6D8EB9B6EA5C477C637295FABF20631C85116BFB";
const VBCABLE_SIGNER_SUBJECT: &str = "BUREL VINCENT Entrepreneur individuel";
const VBCABLE_MAX_ARCHIVE_BYTES: u64 = 5 * 1024 * 1024;
const VBCABLE_MAX_ARCHIVE_FILES: usize = 64;
const VBCABLE_MAX_EXTRACTED_BYTES: u64 = 16 * 1024 * 1024;
const REGISTRY_PATH: &str = r"SOFTWARE\Horuse\Splitwave\Dependencies\VBCable";
const REGISTRY_VALUE: &str = "Manifest";
const HELPER_FLAG: &str = "--vb-cable-helper";
const HELPER_RESULT_PATH_FLAG: &str = "--result-path";
const HELPER_REQUEST_ID_FLAG: &str = "--request-id";
const HELPER_RESULT_MAX_BYTES: u64 = 16 * 1024;
const EXIT_REQUIRES_CONFIRMATION: u32 = 20;
const EXIT_REBOOT_REQUIRED: u32 = 21;

static OPERATION_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

struct OperationGuard;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HelperResult {
    request_id: String,
    success: bool,
    code: String,
    message: String,
    installer_exit_code: Option<i32>,
    render_endpoint_name: Option<String>,
    capture_endpoint_name: Option<String>,
    published_inf: Option<String>,
    ownership: WindowsVirtualCableOwnership,
}

struct HelperResultTarget {
    path: PathBuf,
    request_id: String,
}

struct HelperResultChannel {
    _directory: TemporaryPath,
    path: PathBuf,
    request_id: String,
}

impl OperationGuard {
    fn acquire() -> Result<Self, WindowsVirtualCableError> {
        OPERATION_IN_PROGRESS
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .map_err(|_| {
                WindowsVirtualCableError::new(
                    "operationInProgress",
                    "A VB-CABLE operation is already running",
                )
            })?;
        Ok(Self)
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        OPERATION_IN_PROGRESS.store(false, Ordering::Release);
    }
}

pub fn status() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError> {
    let detected = detect_cable()?;
    let manifest = read_manifest();
    Ok(status_from(
        &detected,
        &manifest,
        &consumer_id(&current_install_root()?)?,
    ))
}

pub fn install() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError> {
    let _guard = OperationGuard::acquire()?;
    let consumer = consumer_id(&current_install_root()?)?;
    let archive = download_archive()?;
    let result_channel = HelperResultChannel::create()?;
    let exit_code = elevate_current(&[
        OsString::from(HELPER_FLAG),
        OsString::from("install"),
        archive.path().as_os_str().to_owned(),
        OsString::from(consumer),
        OsString::from(HELPER_RESULT_PATH_FLAG),
        result_channel.path.as_os_str().to_owned(),
        OsString::from(HELPER_REQUEST_ID_FLAG),
        OsString::from(&result_channel.request_id),
    ])?;
    let helper_result = result_channel.read(exit_code).map_err(|error| {
        warn!(
            stage = "install",
            helper_exit_code = exit_code,
            code = %error.code,
            message = %error.message,
            "VB-CABLE elevated helper result could not be read"
        );
        error
    })?;
    info!(
        stage = "install",
        helper_exit_code = exit_code,
        code = %helper_result.code,
        message = %helper_result.message,
        installer_exit_code = ?helper_result.installer_exit_code,
        render_endpoint = ?helper_result.render_endpoint_name,
        capture_endpoint = ?helper_result.capture_endpoint_name,
        published_inf = ?helper_result.published_inf,
        ownership = ?helper_result.ownership,
        "VB-CABLE elevated helper completed"
    );
    if helper_result.success {
        status()
    } else {
        Err(helper_result.into_error())
    }
}

pub fn run_helper() -> Option<i32> {
    let args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).and_then(|v| v.to_str()) != Some(HELPER_FLAG) {
        return None;
    }
    let action = args.get(2).and_then(|v| v.to_str());
    let root = || {
        args.get(3).map(PathBuf::from).ok_or_else(|| {
            WindowsVirtualCableError::new(
                "invalidHelperArguments",
                "Missing installation directory",
            )
        })
    };
    let result_target = helper_result_target(&args);
    let mut installer_exit_code = None;
    let result = match &result_target {
        Err(error) => Err(error.clone()),
        Ok(_) => match action {
            Some("install") => args
                .get(3)
                .map(PathBuf::from)
                .ok_or_else(|| {
                    WindowsVirtualCableError::new(
                        "invalidHelperArguments",
                        "Missing VB-CABLE archive path",
                    )
                })
                .and_then(|archive| {
                    let consumer = supplied_consumer_id(&args)?;
                    ensure_elevated_or_rerun(&args, || {
                        installer_exit_code = helper_install(&archive, &consumer)?;
                        Ok(())
                    })
                }),
            Some("register-consumer") => {
                root().and_then(|root| register_consumer_with_elevation(&args, &root))
            }
            Some("unregister") => root().and_then(|root| unregister_consumer(&args, &root)),
            Some("retain") => {
                root().and_then(|root| retain_as_external_with_elevation(&args, &root))
            }
            Some("remove") => {
                root().and_then(|root| remove_exact_package_with_elevation(&args, &root))
            }
            _ => Err(WindowsVirtualCableError::new(
                "invalidHelperArguments",
                "Unknown VB-CABLE helper action",
            )),
        },
    };
    let exit_code = match &result {
        Ok(()) => 0,
        Err(error) if error.code == "rebootRequired" => EXIT_REBOOT_REQUIRED,
        Err(error) if error.code == "confirmationRequired" => EXIT_REQUIRES_CONFIRMATION,
        Err(error) => {
            warn!(code = %error.code, message = %error.message, "VB-CABLE helper failed");
            1
        }
    };
    if let Ok(Some(target)) = result_target {
        let helper_result =
            HelperResult::from_operation(target.request_id, &result, installer_exit_code);
        if let Err(error) = write_helper_result(&target.path, &helper_result) {
            warn!(code = %error.code, message = %error.message, "VB-CABLE helper could not write its result");
            return Some(1);
        }
    }
    Some(
        exit_code
            .try_into()
            .expect("fixed helper exit codes fit in i32"),
    )
}

fn ensure_elevated_or_rerun(
    args: &[OsString],
    operation: impl FnOnce() -> Result<(), WindowsVirtualCableError>,
) -> Result<(), WindowsVirtualCableError> {
    if is_elevated()? {
        operation()
    } else {
        let code = elevate_current(&args[1..])?;
        if code == 0 || code == EXIT_REBOOT_REQUIRED {
            Ok(())
        } else {
            Err(helper_error("elevated helper", code))
        }
    }
}

fn unregister_consumer(args: &[OsString], root: &Path) -> Result<(), WindowsVirtualCableError> {
    let detected = detect_cable()?;
    let manifest = read_manifest();
    let consumer = helper_consumer_id(args, root)?;
    let state = determine_removal_state(&detected, &manifest, &consumer);
    // Treat the uninstaller's pending response as "keep" so a sole consumer
    // receives the explicit NSIS prompt instead of silently dropping ownership.
    let action = removal_decision(
        state,
        &manifest,
        &consumer,
        UninstallReason::UserRemoval,
        Some(false),
    );
    match action {
        RemovalAction::Preserve => Ok(()),
        RemovalAction::ReleaseConsumer => {
            ensure_elevated_or_rerun(&args_with_consumer(args, &consumer), || {
                release_consumer(&consumer)
            })
        }
        RemovalAction::RetainAsExternal => Err(WindowsVirtualCableError::confirmation_required()),
        RemovalAction::RemoveExactPackage => Err(WindowsVirtualCableError::new(
            "ownershipConflict",
            "Unexpected VB-CABLE uninstaller state",
        )),
    }
}

fn helper_install(archive: &Path, consumer: &str) -> Result<Option<i32>, WindowsVirtualCableError> {
    let before = detect_cable()?;
    if before.usable() || !before.packages.is_empty() {
        return Err(WindowsVirtualCableError::new(
            "ownershipConflict",
            "VB-CABLE is already present and will not be claimed by Splitwave",
        ));
    }
    let copied = copy_verified_archive(archive)?;
    let extracted = extract_verified_archive(copied.path())?;
    let installer = extracted.path().join(setup_program_name());
    verify_authenticode(&installer)?;

    info!("starting the VB-CABLE vendor installer");
    let result = Command::new(&installer)
        .args(["-i", "-h"])
        .current_dir(extracted.path())
        .status()
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "installerFailed",
                format!("Could not launch VB-CABLE setup: {e}"),
            )
        })?;
    let after = detect_cable().map_err(|error| error.with_installer_exit_code(result.code()))?;
    let package = verified_package_after_setup(&before, &after, result.code())?.clone();
    if !result.success() {
        warn!(
            exit_code = result.code().unwrap_or(-1),
            package = %package.published_name,
            "VB-CABLE setup returned a nonzero code after installing the verified package"
        );
    }
    if !is_exact_package_name(&package.published_name) {
        return Err(WindowsVirtualCableError::new(
            "driverPackageNotFound",
            "VB-CABLE setup did not expose a safe published driver package name",
        )
        .with_installer_exit_code(result.code()));
    }

    let manifest = OwnershipManifest {
        schema_version: MANIFEST_SCHEMA_VERSION,
        provider: PROVIDER_NAME.into(),
        ownership: WindowsVirtualCableOwnership::Managed,
        installed_at: now_unix_seconds(),
        installed_version: package.version.clone(),
        installer_source: VBCABLE_URL.into(),
        installer_sha256: VBCABLE_ARCHIVE_SHA256.into(),
        installer_publisher: VBCABLE_SIGNER_SUBJECT.into(),
        package_published_names: vec![package.published_name.clone()],
        package_original_names: package.original_name.clone().into_iter().collect(),
        package_fingerprint: package.fingerprint(),
        device_instance_ids: package.device_instance_ids.clone(),
        consumer_installation_ids: vec![consumer.into()],
        // VB-Audio's current reference manual requires a restart after installation.
        pending_reboot: !after.usable(),
        removal_pending_reboot: false,
    };
    write_manifest(&manifest).map_err(|error| error.with_installer_exit_code(result.code()))?;
    if !after.usable() {
        return Err(WindowsVirtualCableError::new("rebootRequired", "VB-CABLE was installed, but Windows must be restarted before both endpoints are available").with_installer_exit_code(result.code()));
    }
    Ok(result.code())
}

fn register_consumer(consumer: &str) -> Result<(), WindowsVirtualCableError> {
    let detected = detect_cable()?;
    let ManifestState::Valid(mut manifest) = read_manifest() else {
        return Ok(());
    };
    let Some(package) = detected.package() else {
        return Ok(());
    };
    if !manifest.matches(package) {
        return Ok(());
    }
    if !manifest
        .consumer_installation_ids
        .iter()
        .any(|id| id == consumer)
    {
        manifest.consumer_installation_ids.push(consumer.into());
        write_manifest(&manifest)?;
    }
    Ok(())
}

fn register_consumer_with_elevation(
    args: &[OsString],
    root: &Path,
) -> Result<(), WindowsVirtualCableError> {
    let detected = detect_cable()?;
    let ManifestState::Valid(manifest) = read_manifest() else {
        return Ok(());
    };
    let Some(package) = detected.package() else {
        return Ok(());
    };
    let consumer = helper_consumer_id(args, root)?;
    if !manifest.matches(package)
        || manifest
            .consumer_installation_ids
            .iter()
            .any(|id| id == &consumer)
    {
        return Ok(());
    }
    ensure_elevated_or_rerun(&args_with_consumer(args, &consumer), || {
        register_consumer(&consumer)
    })
}

fn release_consumer(consumer: &str) -> Result<(), WindowsVirtualCableError> {
    let ManifestState::Valid(mut manifest) = read_manifest() else {
        return Ok(());
    };
    manifest
        .consumer_installation_ids
        .retain(|id| id != consumer);
    write_manifest(&manifest)
}

fn retain_as_external(consumer: &str) -> Result<(), WindowsVirtualCableError> {
    let detected = detect_cable()?;
    let ManifestState::Valid(mut manifest) = read_manifest() else {
        return Ok(());
    };
    let state =
        determine_removal_state(&detected, &ManifestState::Valid(manifest.clone()), consumer);
    if removal_decision(
        state,
        &ManifestState::Valid(manifest.clone()),
        consumer,
        UninstallReason::UserRemoval,
        Some(false),
    ) != RemovalAction::RetainAsExternal
    {
        return Err(WindowsVirtualCableError::new(
            "ownershipConflict",
            "VB-CABLE ownership changed before Splitwave could retain it",
        ));
    }
    manifest.consumer_installation_ids.clear();
    manifest.ownership = WindowsVirtualCableOwnership::External;
    manifest.pending_reboot = false;
    write_manifest(&manifest)
}

fn retain_as_external_with_elevation(
    args: &[OsString],
    root: &Path,
) -> Result<(), WindowsVirtualCableError> {
    let consumer = helper_consumer_id(args, root)?;
    ensure_elevated_or_rerun(&args_with_consumer(args, &consumer), || {
        retain_as_external(&consumer)
    })
}

fn remove_exact_package_with_elevation(
    args: &[OsString],
    root: &Path,
) -> Result<(), WindowsVirtualCableError> {
    let consumer = helper_consumer_id(args, root)?;
    ensure_elevated_or_rerun(&args_with_consumer(args, &consumer), || {
        remove_exact_package(&consumer)
    })
}

fn remove_exact_package(consumer: &str) -> Result<(), WindowsVirtualCableError> {
    let detected = detect_cable()?;
    let ManifestState::Valid(manifest) = read_manifest() else {
        return Err(WindowsVirtualCableError::new(
            "ownershipConflict",
            "No managed VB-CABLE ownership record exists",
        ));
    };
    let state =
        determine_removal_state(&detected, &ManifestState::Valid(manifest.clone()), consumer);
    if removal_decision(
        state,
        &ManifestState::Valid(manifest.clone()),
        consumer,
        UninstallReason::UserRemoval,
        Some(true),
    ) != RemovalAction::RemoveExactPackage
    {
        return Err(WindowsVirtualCableError::new(
            "ownershipConflict",
            "VB-CABLE no longer matches Splitwave's managed package record",
        ));
    }
    let published = manifest.package_published_names.first().ok_or_else(|| {
        WindowsVirtualCableError::new(
            "driverPackageNotFound",
            "Managed VB-CABLE record has no published INF name",
        )
    })?;
    if !is_exact_package_name(published) {
        return Err(WindowsVirtualCableError::new(
            "packageFingerprintMismatch",
            "Managed VB-CABLE record contains an unsafe package name",
        ));
    }
    let status = Command::new("pnputil.exe")
        .args(["/delete-driver", published, "/uninstall"])
        .status()
        .map_err(|e| {
            WindowsVirtualCableError::new("removalFailed", format!("Could not start PnPUtil: {e}"))
        })?;
    match status.code() {
        Some(0) => {
            let after = detect_cable()?;
            let reboot_required = after
                .packages
                .iter()
                .any(|p| p.published_name.eq_ignore_ascii_case(published));
            delete_manifest()?;
            if reboot_required {
                Err(WindowsVirtualCableError::new(
                    "rebootRequired",
                    "VB-CABLE removal is pending a Windows restart",
                ))
            } else {
                Ok(())
            }
        }
        Some(3010) => {
            delete_manifest()?;
            Err(WindowsVirtualCableError::new(
                "rebootRequired",
                "VB-CABLE removal is pending a Windows restart",
            ))
        }
        Some(code) => Err(WindowsVirtualCableError::new(
            "removalFailed",
            format!("PnPUtil exited with {code}"),
        )),
        None => Err(WindowsVirtualCableError::new(
            "removalFailed",
            "PnPUtil terminated unexpectedly",
        )),
    }
}

fn detect_cable() -> Result<DetectedCable, WindowsVirtualCableError> {
    let packages = enumerate_vb_cable_packages()?;
    let endpoint_parents = enumerate_audio_endpoint_parents()?;
    let endpoints = enumerate_active_audio_endpoints(&endpoint_parents)?;
    Ok(detect_cable_from_inventory(packages, endpoints))
}

fn ensure_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn enumerate_audio_endpoint_parents() -> Result<HashMap<String, String>, WindowsVirtualCableError> {
    let set = unsafe { SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES) }
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "deviceEnumerationFailed",
                format!("SetupAPI device list: {e}"),
            )
        })?;
    let _set = DeviceSet(set);
    let mut parents = HashMap::new();
    for index in 0.. {
        let mut data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if let Err(error) = unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) } {
            if (error.code().0 as u32 & 0xffff) == ERROR_NO_MORE_ITEMS.0 {
                break;
            }
            return Err(WindowsVirtualCableError::new(
                "deviceEnumerationFailed",
                format!("SetupAPI enumerate: {error}"),
            ));
        }
        let Some(instance_id) = device_instance_id(set, &data) else {
            continue;
        };
        if !instance_id
            .to_ascii_lowercase()
            .starts_with("swd\\mmdevapi\\")
        {
            continue;
        }
        let Some(parent) = property_string(set, &data, &DEVPKEY_Device_Parent) else {
            continue;
        };
        parents.insert(instance_id.to_ascii_lowercase(), parent);
    }
    Ok(parents)
}

fn enumerate_active_audio_endpoints(
    endpoint_parents: &HashMap<String, String>,
) -> Result<Vec<CableEndpoint>, WindowsVirtualCableError> {
    ensure_com();
    let enumerator: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.map_err(|e| {
            WindowsVirtualCableError::new(
                "deviceEnumerationFailed",
                format!("MMDevice enumerator: {e}"),
            )
        })?;
    let mut endpoints = Vec::new();
    for (flow, endpoint_flow) in [
        (eRender, CableEndpointFlow::Render),
        (eCapture, CableEndpointFlow::Capture),
    ] {
        let collection = unsafe { enumerator.EnumAudioEndpoints(flow, DEVICE_STATE_ACTIVE) }
            .map_err(|e| {
                WindowsVirtualCableError::new(
                    "deviceEnumerationFailed",
                    format!("MMDevice endpoint collection: {e}"),
                )
            })?;
        let count = unsafe { collection.GetCount() }.map_err(|e| {
            WindowsVirtualCableError::new(
                "deviceEnumerationFailed",
                format!("MMDevice endpoint count: {e}"),
            )
        })?;
        for index in 0..count {
            let device = unsafe { collection.Item(index) }.map_err(|e| {
                WindowsVirtualCableError::new(
                    "deviceEnumerationFailed",
                    format!("MMDevice endpoint: {e}"),
                )
            })?;
            let Some(endpoint_id) = endpoint_id(&device) else {
                continue;
            };
            let pnp_id = format!("SWD\\MMDEVAPI\\{endpoint_id}").to_ascii_lowercase();
            let Some(parent_instance_id) = endpoint_parents.get(&pnp_id) else {
                continue;
            };
            let Some(name) = endpoint_name(&device) else {
                continue;
            };
            endpoints.push(CableEndpoint {
                flow: endpoint_flow,
                name,
                parent_instance_id: parent_instance_id.clone(),
            });
        }
    }
    Ok(endpoints)
}

fn endpoint_id(device: &IMMDevice) -> Option<String> {
    let id = unsafe { device.GetId().ok()? };
    let value = unsafe { id.to_string().ok() };
    unsafe {
        CoTaskMemFree(Some(id.0.cast()));
    }
    value
}

fn endpoint_name(device: &IMMDevice) -> Option<String> {
    let store = unsafe { device.OpenPropertyStore(STGM_READ).ok()? };
    let mut prop = unsafe { store.GetValue(&PKEY_Device_FriendlyName).ok()? };
    let name = unsafe { prop.Anonymous.Anonymous.Anonymous.pwszVal.to_string().ok() }
        .filter(|name| !name.is_empty());
    unsafe {
        let _ = PropVariantClear(&mut prop);
    }
    name
}

fn enumerate_vb_cable_packages() -> Result<Vec<CablePackage>, WindowsVirtualCableError> {
    let set = unsafe { SetupDiGetClassDevsW(None, PCWSTR::null(), None, DIGCF_ALLCLASSES) }
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "deviceEnumerationFailed",
                format!("SetupAPI device list: {e}"),
            )
        })?;
    let _set = DeviceSet(set);
    let mut packages: Vec<CablePackage> = Vec::new();
    for index in 0.. {
        let mut data = SP_DEVINFO_DATA {
            cbSize: std::mem::size_of::<SP_DEVINFO_DATA>() as u32,
            ..Default::default()
        };
        if let Err(error) = unsafe { SetupDiEnumDeviceInfo(set, index, &mut data) } {
            if (error.code().0 as u32 & 0xffff) == ERROR_NO_MORE_ITEMS.0 {
                break;
            }
            return Err(WindowsVirtualCableError::new(
                "deviceEnumerationFailed",
                format!("SetupAPI enumerate: {error}"),
            ));
        }
        let provider = property_string(set, &data, &DEVPKEY_Device_DriverProvider);
        let published_name = property_string(set, &data, &DEVPKEY_Device_DriverInfPath);
        // Root media devices do not consistently expose FriendlyName through
        // SetupAPI. DeviceDesc carries the same signed-driver identity on those
        // systems, so use it only as a fallback and retain the provider/package
        // checks below. This still excludes VB-Audio's Voicemeeter packages.
        let device_name = property_string(set, &data, &DEVPKEY_Device_FriendlyName)
            .or_else(|| property_string(set, &data, &DEVPKEY_Device_DeviceDesc));
        let (Some(provider), Some(published_name), Some(device_name)) =
            (provider, published_name, device_name)
        else {
            continue;
        };
        if !is_vb_audio_provider(&provider)
            || !is_vb_cable_friendly_name(&device_name)
            || !is_exact_package_name(&published_name)
        {
            continue;
        }
        let version = property_string(set, &data, &DEVPKEY_Device_DriverVersion);
        let instance_id = device_instance_id(set, &data).unwrap_or_default();
        if let Some(existing) = packages
            .iter_mut()
            .find(|p| p.published_name.eq_ignore_ascii_case(&published_name))
        {
            if !instance_id.is_empty() && !existing.device_instance_ids.contains(&instance_id) {
                existing.device_instance_ids.push(instance_id);
            }
        } else {
            packages.push(CablePackage {
                provider,
                version,
                published_name,
                original_name: None,
                device_instance_ids: (!instance_id.is_empty())
                    .then_some(instance_id)
                    .into_iter()
                    .collect(),
            });
        }
    }
    Ok(packages)
}

struct DeviceSet(windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO);

impl Drop for DeviceSet {
    fn drop(&mut self) {
        unsafe {
            let _ = SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

fn property_string(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &SP_DEVINFO_DATA,
    key: &windows::Win32::Foundation::DEVPROPKEY,
) -> Option<String> {
    let mut property_type = Default::default();
    let mut required = 0;
    unsafe {
        let _ = SetupDiGetDevicePropertyW(
            set,
            data,
            key,
            &mut property_type,
            None,
            Some(&mut required),
            0,
        );
    }
    if required < 2 || required > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0_u8; required as usize];
    unsafe {
        SetupDiGetDevicePropertyW(
            set,
            data,
            key,
            &mut property_type,
            Some(&mut buffer),
            Some(&mut required),
            0,
        )
        .ok()?;
    }
    let utf16: Vec<u16> = buffer
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let end = utf16.iter().position(|&v| v == 0).unwrap_or(utf16.len());
    String::from_utf16(&utf16[..end])
        .ok()
        .filter(|s| !s.is_empty())
}

fn device_instance_id(
    set: windows::Win32::Devices::DeviceAndDriverInstallation::HDEVINFO,
    data: &SP_DEVINFO_DATA,
) -> Option<String> {
    let mut required = 0;
    unsafe {
        let _ = SetupDiGetDeviceInstanceIdW(set, data, None, Some(&mut required));
    }
    if required < 2 || required > 64 * 1024 {
        return None;
    }
    let mut buffer = vec![0_u16; required as usize];
    unsafe {
        SetupDiGetDeviceInstanceIdW(set, data, Some(&mut buffer), Some(&mut required)).ok()?;
    }
    let end = buffer.iter().position(|&v| v == 0).unwrap_or(buffer.len());
    String::from_utf16(&buffer[..end])
        .ok()
        .filter(|s| !s.is_empty())
}

fn is_vb_audio_provider(provider: &str) -> bool {
    let value = provider.to_ascii_lowercase();
    value.contains("burel vincent") || value.contains("vb-audio")
}

fn is_vb_cable_friendly_name(name: &str) -> bool {
    let value = name.to_ascii_lowercase();
    value.contains("cable") && !value.contains("voicemeeter")
}

fn is_exact_package_name(name: &str) -> bool {
    let value = name.to_ascii_lowercase();
    if !value.starts_with("oem") || !value.ends_with(".inf") {
        return false;
    }
    let digits = &value[3..value.len() - 4];
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

fn read_manifest() -> ManifestState {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let Ok(key) = hklm.open_subkey_with_flags(REGISTRY_PATH, KEY_READ | KEY_WOW64_64KEY) else {
        return ManifestState::Missing;
    };
    let Ok(raw) = key.get_value::<String, _>(REGISTRY_VALUE) else {
        return ManifestState::Corrupt;
    };
    match serde_json::from_str::<OwnershipManifest>(&raw) {
        Ok(manifest) if manifest.schema_version == MANIFEST_SCHEMA_VERSION => {
            ManifestState::Valid(manifest)
        }
        _ => ManifestState::Corrupt,
    }
}

fn write_manifest(manifest: &OwnershipManifest) -> Result<(), WindowsVirtualCableError> {
    let raw = serde_json::to_string(manifest).map_err(|e| {
        WindowsVirtualCableError::new(
            "ownershipWriteFailed",
            format!("Serialize ownership manifest: {e}"),
        )
    })?;
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let (key, _) = hklm
        .create_subkey_with_flags(REGISTRY_PATH, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "ownershipWriteFailed",
                format!("Open machine ownership store: {e}"),
            )
        })?;
    key.set_value(REGISTRY_VALUE, &raw).map_err(|e| {
        WindowsVirtualCableError::new(
            "ownershipWriteFailed",
            format!("Write machine ownership store: {e}"),
        )
    })
}

fn delete_manifest() -> Result<(), WindowsVirtualCableError> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey_with_flags(REGISTRY_PATH, KEY_WRITE | KEY_WOW64_64KEY)
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "ownershipWriteFailed",
                format!("Open machine ownership store: {e}"),
            )
        })?;
    let _ = key.delete_value(REGISTRY_VALUE);
    Ok(())
}

struct TemporaryPath(PathBuf);

impl TemporaryPath {
    fn create_dir(prefix: &str) -> Result<Self, WindowsVirtualCableError> {
        for _ in 0..10 {
            let path =
                std::env::temp_dir().join(format!("splitwave-{prefix}-{}", cuid2::create_id()));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(WindowsVirtualCableError::new(
                        "temporaryStorageFailed",
                        format!("Create temporary directory: {error}"),
                    ));
                }
            }
        }
        Err(WindowsVirtualCableError::new(
            "temporaryStorageFailed",
            "Could not allocate a unique temporary directory",
        ))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TemporaryPath {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct TemporaryArchive {
    _dir: TemporaryPath,
    file: PathBuf,
}

impl TemporaryArchive {
    fn path(&self) -> &Path {
        &self.file
    }
}

fn create_temporary_archive(
    dir: TemporaryPath,
) -> Result<(TemporaryArchive, PathBuf, File), WindowsVirtualCableError> {
    let archive = TemporaryArchive {
        file: dir.path().join("VBCABLE_Driver_Pack45.zip"),
        _dir: dir,
    };
    let part = archive.file.with_extension("zip.part");
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "temporaryStorageFailed",
                format!("Create downloaded archive: {e}"),
            )
        })?;
    Ok((archive, part, file))
}

fn finalize_temporary_archive(
    archive: TemporaryArchive,
    part: &Path,
) -> Result<TemporaryArchive, WindowsVirtualCableError> {
    fs::rename(part, &archive.file).map_err(|e| {
        WindowsVirtualCableError::new(
            "temporaryStorageFailed",
            format!("Finalize downloaded archive: {e}"),
        )
    })?;
    Ok(archive)
}

impl HelperResult {
    fn from_operation(
        request_id: String,
        result: &Result<(), WindowsVirtualCableError>,
        installer_exit_code: Option<i32>,
    ) -> Self {
        let detected = detect_cable().unwrap_or_default();
        let manifest = read_manifest();
        let published_inf = detected
            .package()
            .map(|package| package.published_name.clone());
        let (success, code, message, installer_exit_code) = match result {
            Ok(()) => (
                true,
                "ok".into(),
                "VB-CABLE helper completed".into(),
                installer_exit_code,
            ),
            Err(error) => (
                error.code == "rebootRequired",
                error.code.clone(),
                error.message.clone(),
                error.installer_exit_code.or(installer_exit_code),
            ),
        };
        Self {
            request_id,
            success,
            code,
            message,
            installer_exit_code,
            render_endpoint_name: detected.render_endpoint_name,
            capture_endpoint_name: detected.capture_endpoint_name,
            published_inf,
            ownership: manifest_ownership(&manifest),
        }
    }

    fn into_error(self) -> WindowsVirtualCableError {
        WindowsVirtualCableError::new(self.code, self.message)
            .with_installer_exit_code(self.installer_exit_code)
    }
}

impl HelperResultChannel {
    fn create() -> Result<Self, WindowsVirtualCableError> {
        let directory = TemporaryPath::create_dir("vb-cable-result")?;
        let path = directory.path().join("result.json");
        Ok(Self {
            _directory: directory,
            path,
            request_id: cuid2::create_id(),
        })
    }

    fn read(&self, helper_exit_code: u32) -> Result<HelperResult, WindowsVirtualCableError> {
        read_helper_result(&self.path, &self.request_id, helper_exit_code)
    }
}

fn helper_result_target(
    args: &[OsString],
) -> Result<Option<HelperResultTarget>, WindowsVirtualCableError> {
    let path_index = args
        .iter()
        .position(|value| value.to_str() == Some(HELPER_RESULT_PATH_FLAG));
    let request_index = args
        .iter()
        .position(|value| value.to_str() == Some(HELPER_REQUEST_ID_FLAG));
    let (Some(path_index), Some(request_index)) = (path_index, request_index) else {
        if path_index.is_none() && request_index.is_none() {
            return Ok(None);
        }
        return Err(WindowsVirtualCableError::new(
            "invalidHelperArguments",
            "Incomplete helper result arguments",
        ));
    };
    let path = args.get(path_index + 1).map(PathBuf::from).ok_or_else(|| {
        WindowsVirtualCableError::new("invalidHelperArguments", "Missing helper result path")
    })?;
    let request_id = args
        .get(request_index + 1)
        .and_then(|value| value.to_str())
        .filter(|value| valid_request_id(value))
        .ok_or_else(|| {
            WindowsVirtualCableError::new(
                "invalidHelperArguments",
                "Invalid helper request identifier",
            )
        })?;
    Ok(Some(HelperResultTarget {
        path,
        request_id: request_id.into(),
    }))
}

fn valid_request_id(value: &str) -> bool {
    (16..=64).contains(&value.len())
        && value
            .chars()
            .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit())
}

fn valid_result_code(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .chars()
            .all(|character| character.is_ascii_alphanumeric())
}

fn write_helper_result(path: &Path, result: &HelperResult) -> Result<(), WindowsVirtualCableError> {
    let bytes = serde_json::to_vec(result).map_err(|error| {
        WindowsVirtualCableError::new(
            "helperResultWriteFailed",
            format!("Serialize helper result: {error}"),
        )
    })?;
    if bytes.len() as u64 > HELPER_RESULT_MAX_BYTES {
        return Err(WindowsVirtualCableError::new(
            "helperResultWriteFailed",
            "Helper result exceeds the allowed size",
        ));
    }
    let part = path.with_extension("json.part");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part)
        .map_err(|error| {
            WindowsVirtualCableError::new(
                "helperResultWriteFailed",
                format!("Create helper result: {error}"),
            )
        })?;
    file.write_all(&bytes).map_err(|error| {
        WindowsVirtualCableError::new(
            "helperResultWriteFailed",
            format!("Write helper result: {error}"),
        )
    })?;
    file.sync_all().map_err(|error| {
        WindowsVirtualCableError::new(
            "helperResultWriteFailed",
            format!("Flush helper result: {error}"),
        )
    })?;
    drop(file);
    fs::rename(&part, path).map_err(|error| {
        WindowsVirtualCableError::new(
            "helperResultWriteFailed",
            format!("Publish helper result: {error}"),
        )
    })
}

fn read_helper_result(
    path: &Path,
    request_id: &str,
    helper_exit_code: u32,
) -> Result<HelperResult, WindowsVirtualCableError> {
    let metadata = fs::metadata(path).map_err(|error| {
        warn!(helper_exit_code, error = %error, "VB-CABLE helper result is missing");
        WindowsVirtualCableError::new(
            "helperResultMissing",
            "VB-CABLE helper did not return a diagnostic result",
        )
    })?;
    if !metadata.is_file() || metadata.len() > HELPER_RESULT_MAX_BYTES {
        let _ = fs::remove_file(path);
        return Err(WindowsVirtualCableError::new(
            "helperResultInvalid",
            "VB-CABLE helper returned an invalid diagnostic result",
        ));
    }
    let bytes = fs::read(path).map_err(|error| {
        WindowsVirtualCableError::new(
            "helperResultInvalid",
            format!("Read helper result: {error}"),
        )
    });
    let _ = fs::remove_file(path);
    let bytes = bytes?;
    let result: HelperResult = serde_json::from_slice(&bytes).map_err(|error| {
        WindowsVirtualCableError::new(
            "helperResultInvalid",
            format!("Parse helper result: {error}"),
        )
    })?;
    if result.request_id != request_id
        || !valid_request_id(&result.request_id)
        || !valid_result_code(&result.code)
        || result.message.is_empty()
        || result.message.len() > 2048
    {
        return Err(WindowsVirtualCableError::new(
            "helperResultInvalid",
            "VB-CABLE helper returned an invalid diagnostic result",
        ));
    }
    let expected_exit_code = if result.success {
        if result.code == "rebootRequired" {
            EXIT_REBOOT_REQUIRED
        } else {
            0
        }
    } else if result.code == "confirmationRequired" {
        EXIT_REQUIRES_CONFIRMATION
    } else {
        1
    };
    if helper_exit_code != expected_exit_code {
        return Err(WindowsVirtualCableError::new(
            "helperResultInvalid",
            "VB-CABLE helper result did not match the process exit code",
        ));
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST_ID: &str = "0123456789abcdef01234567";

    fn helper_result(success: bool, code: &str) -> HelperResult {
        HelperResult {
            request_id: REQUEST_ID.into(),
            success,
            code: code.into(),
            message: "helper message".into(),
            installer_exit_code: None,
            render_endpoint_name: Some("CABLE Input".into()),
            capture_endpoint_name: Some("CABLE Output".into()),
            published_inf: Some("oem42.inf".into()),
            ownership: WindowsVirtualCableOwnership::Managed,
        }
    }

    fn result_file(prefix: &str) -> Result<(TemporaryPath, PathBuf), WindowsVirtualCableError> {
        let directory = TemporaryPath::create_dir(prefix)?;
        let path = directory.path().join("result.json");
        Ok((directory, path))
    }

    #[test]
    fn cable_device_name_excludes_voicemeeter() {
        assert!(is_vb_cable_friendly_name("VB-Audio Virtual Cable"));
        assert!(is_vb_cable_friendly_name("CABLE Input"));
        assert!(!is_vb_cable_friendly_name("VB-Audio VoiceMeeter VAIO"));
    }

    #[test]
    fn temporary_archive_is_published_only_after_rename() -> Result<(), WindowsVirtualCableError> {
        let (archive, part, mut file) =
            create_temporary_archive(TemporaryPath::create_dir("vb-cable-test")?)?;
        assert!(!archive.path().exists());
        assert!(part.is_file());

        file.write_all(b"archive").unwrap();
        file.sync_all().unwrap();
        drop(file);

        let archive = finalize_temporary_archive(archive, &part)?;
        assert!(archive.path().is_file());
        assert!(!part.exists());
        Ok(())
    }

    #[test]
    fn supplied_consumer_id_requires_exact_hash_format() {
        let valid = OsString::from("splitwave-0123456789abcdef01234567");
        let args = [
            OsString::from("splitwave.exe"),
            OsString::from(HELPER_FLAG),
            OsString::from("remove"),
            OsString::from("C:\\Splitwave"),
            valid.clone(),
        ];
        assert_eq!(
            supplied_consumer_id(&args).unwrap(),
            valid.to_string_lossy()
        );

        let invalid = OsString::from("splitwave-not-a-valid-consumer");
        let mut args = args;
        args[4] = invalid;
        assert_eq!(
            supplied_consumer_id(&args).unwrap_err().code,
            "invalidHelperArguments"
        );
    }

    #[test]
    fn helper_result_round_trips_and_is_removed() -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-round-trip")?;
        let expected = helper_result(true, "ok");
        write_helper_result(&path, &expected)?;

        let actual = read_helper_result(&path, REQUEST_ID, 0)?;

        assert_eq!(actual, expected);
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn helper_result_preserves_unknown_error_code() -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-unknown-code")?;
        let expected = helper_result(false, "newVendorFailure");
        write_helper_result(&path, &expected)?;

        let actual = read_helper_result(&path, REQUEST_ID, 1)?;

        assert_eq!(actual.code, "newVendorFailure");
        Ok(())
    }

    #[test]
    fn missing_helper_result_is_reported() -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-missing")?;

        let error = read_helper_result(&path, REQUEST_ID, 1).unwrap_err();

        assert_eq!(error.code, "helperResultMissing");
        Ok(())
    }

    #[test]
    fn corrupt_helper_result_is_rejected_and_removed() -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-corrupt")?;
        fs::write(&path, b"{").unwrap();

        let error = read_helper_result(&path, REQUEST_ID, 1).unwrap_err();

        assert_eq!(error.code, "helperResultInvalid");
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn oversized_helper_result_is_rejected_and_removed() -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-oversized")?;
        fs::write(&path, vec![b'x'; HELPER_RESULT_MAX_BYTES as usize + 1]).unwrap();

        let error = read_helper_result(&path, REQUEST_ID, 1).unwrap_err();

        assert_eq!(error.code, "helperResultInvalid");
        assert!(!path.exists());
        Ok(())
    }

    #[test]
    fn uac_cancellation_remains_distinct() {
        let error = helper_error("install", ERROR_CANCELLED.0);
        assert_eq!(error.code, "elevationCancelled");
    }

    #[test]
    fn reboot_required_result_matches_reserved_exit_code() -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-reboot")?;
        let expected = helper_result(true, "rebootRequired");
        write_helper_result(&path, &expected)?;

        let actual = read_helper_result(&path, REQUEST_ID, EXIT_REBOOT_REQUIRED)?;

        assert!(actual.success);
        assert_eq!(actual.code, "rebootRequired");
        Ok(())
    }

    #[test]
    fn installer_exit_code_is_preserved_with_specific_failure(
    ) -> Result<(), WindowsVirtualCableError> {
        let (_directory, path) = result_file("vb-cable-result-installer-exit")?;
        let mut expected = helper_result(false, "driverPackageNotDetected");
        expected.installer_exit_code = Some(1);
        expected.message =
            "VB-CABLE installer finished, but Windows did not register the expected driver".into();
        write_helper_result(&path, &expected)?;

        let error = read_helper_result(&path, REQUEST_ID, 1)?.into_error();

        assert_eq!(error.code, "driverPackageNotDetected");
        assert_eq!(error.installer_exit_code, Some(1));
        Ok(())
    }
}

fn download_archive() -> Result<TemporaryArchive, WindowsVirtualCableError> {
    let parsed = reqwest::Url::parse(VBCABLE_URL).map_err(|_| {
        WindowsVirtualCableError::new("unexpectedDownloadHost", "VB-CABLE source URL is invalid")
    })?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("download.vb-audio.com") {
        return Err(WindowsVirtualCableError::new(
            "unexpectedDownloadHost",
            "VB-CABLE source is not the approved VB-Audio HTTPS host",
        ));
    }
    let client = Client::builder()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.url().scheme() == "https"
                && attempt.url().host_str() == Some("download.vb-audio.com")
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|e| {
            WindowsVirtualCableError::new("downloadFailed", format!("Create download client: {e}"))
        })?;
    let mut response = client
        .get(VBCABLE_URL)
        .send()
        .and_then(|response| response.error_for_status())
        .map_err(|e| {
            WindowsVirtualCableError::new("downloadFailed", format!("Download VB-CABLE: {e}"))
        })?;
    if response
        .content_length()
        .is_some_and(|size| size > VBCABLE_MAX_ARCHIVE_BYTES)
    {
        return Err(WindowsVirtualCableError::new(
            "archiveInvalid",
            "VB-CABLE archive exceeds the allowed download size",
        ));
    }
    let (archive, part, mut out) =
        create_temporary_archive(TemporaryPath::create_dir("vb-cable-download")?)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buf = [0_u8; 32 * 1024];
    loop {
        let read = response.read(&mut buf).map_err(|e| {
            WindowsVirtualCableError::new("downloadFailed", format!("Read VB-CABLE archive: {e}"))
        })?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > VBCABLE_MAX_ARCHIVE_BYTES {
            return Err(WindowsVirtualCableError::new(
                "archiveInvalid",
                "VB-CABLE archive exceeds the allowed download size",
            ));
        }
        out.write_all(&buf[..read]).map_err(|e| {
            WindowsVirtualCableError::new(
                "temporaryStorageFailed",
                format!("Write downloaded archive: {e}"),
            )
        })?;
        hasher.update(&buf[..read]);
    }
    out.sync_all().map_err(|e| {
        WindowsVirtualCableError::new(
            "temporaryStorageFailed",
            format!("Finalize downloaded archive: {e}"),
        )
    })?;
    drop(out);
    verify_hash(&hasher.finalize())?;
    finalize_temporary_archive(archive, &part)
}

fn copy_verified_archive(source: &Path) -> Result<TemporaryArchive, WindowsVirtualCableError> {
    let mut input = File::open(source).map_err(|e| {
        WindowsVirtualCableError::new("archiveInvalid", format!("Open VB-CABLE archive: {e}"))
    })?;
    let (archive, part, mut out) =
        create_temporary_archive(TemporaryPath::create_dir("vb-cable-helper")?)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buf = [0_u8; 32 * 1024];
    loop {
        let read = input.read(&mut buf).map_err(|e| {
            WindowsVirtualCableError::new("archiveInvalid", format!("Read VB-CABLE archive: {e}"))
        })?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > VBCABLE_MAX_ARCHIVE_BYTES {
            return Err(WindowsVirtualCableError::new(
                "archiveInvalid",
                "VB-CABLE archive exceeds the allowed download size",
            ));
        }
        out.write_all(&buf[..read]).map_err(|e| {
            WindowsVirtualCableError::new(
                "temporaryStorageFailed",
                format!("Write helper archive: {e}"),
            )
        })?;
        hasher.update(&buf[..read]);
    }
    out.sync_all().map_err(|e| {
        WindowsVirtualCableError::new(
            "temporaryStorageFailed",
            format!("Finalize helper archive: {e}"),
        )
    })?;
    drop(out);
    verify_hash(&hasher.finalize())?;
    finalize_temporary_archive(archive, &part)
}

fn verify_hash(digest: &[u8]) -> Result<(), WindowsVirtualCableError> {
    let actual = digest
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<String>();
    if actual == VBCABLE_ARCHIVE_SHA256 {
        Ok(())
    } else {
        Err(WindowsVirtualCableError::new(
            "checksumMismatch",
            "The downloaded VB-CABLE archive does not match Splitwave's approved SHA-256",
        ))
    }
}

fn extract_verified_archive(archive: &Path) -> Result<TemporaryPath, WindowsVirtualCableError> {
    let file = File::open(archive).map_err(|e| {
        WindowsVirtualCableError::new("archiveInvalid", format!("Open verified archive: {e}"))
    })?;
    let mut zip = ZipArchive::new(file).map_err(|e| {
        WindowsVirtualCableError::new("archiveInvalid", format!("Read VB-CABLE archive: {e}"))
    })?;
    if zip.len() > VBCABLE_MAX_ARCHIVE_FILES {
        return Err(WindowsVirtualCableError::new(
            "archiveInvalid",
            "VB-CABLE archive contains too many files",
        ));
    }
    let dir = TemporaryPath::create_dir("vb-cable-extract")?;
    let mut total = 0_u64;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|e| {
            WindowsVirtualCableError::new(
                "archiveInvalid",
                format!("Read VB-CABLE archive entry: {e}"),
            )
        })?;
        let Some(name) = entry.enclosed_name().map(PathBuf::from) else {
            return Err(WindowsVirtualCableError::new(
                "archiveInvalid",
                "VB-CABLE archive contains an unsafe path",
            ));
        };
        if entry.is_dir() {
            continue;
        }
        total += entry.size();
        if total > VBCABLE_MAX_EXTRACTED_BYTES {
            return Err(WindowsVirtualCableError::new(
                "archiveInvalid",
                "VB-CABLE archive expands beyond the allowed size",
            ));
        }
        let output = dir.path().join(name.file_name().ok_or_else(|| {
            WindowsVirtualCableError::new(
                "archiveInvalid",
                "VB-CABLE archive entry has no file name",
            )
        })?);
        let mut output_file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)
            .map_err(|e| {
                WindowsVirtualCableError::new(
                    "temporaryStorageFailed",
                    format!("Extract VB-CABLE archive: {e}"),
                )
            })?;
        std::io::copy(&mut entry, &mut output_file).map_err(|e| {
            WindowsVirtualCableError::new(
                "archiveInvalid",
                format!("Extract VB-CABLE archive entry: {e}"),
            )
        })?;
    }
    if !dir.path().join(setup_program_name()).is_file() {
        return Err(WindowsVirtualCableError::new(
            "installerNotFound",
            "VB-CABLE archive does not contain the setup program for this architecture",
        ));
    }
    Ok(dir)
}

fn setup_program_name() -> &'static str {
    if cfg!(target_arch = "x86") {
        "VBCABLE_Setup.exe"
    } else {
        // VB-Audio's current x64 setup supports both x64 Windows and Windows on Arm64.
        "VBCABLE_Setup_x64.exe"
    }
}

fn verify_authenticode(installer: &Path) -> Result<(), WindowsVirtualCableError> {
    const SCRIPT: &str = r#"param([Parameter(Mandatory = $true)][string]$InstallerPath)
$signature = Get-AuthenticodeSignature -LiteralPath $InstallerPath
Write-Output $signature.Status
if ($signature.Status -ne 'Valid') { exit 2 }
if ($signature.SignerCertificate.GetNameInfo([System.Security.Cryptography.X509Certificates.X509NameType]::SimpleName, $false) -ne 'BUREL VINCENT Entrepreneur individuel') { exit 3 }
if ($signature.SignerCertificate.Thumbprint -ne 'A77952D93229D0EC36E2543081EEA7D125732B9C') { exit 4 }
exit 0
"#;
    let script_dir = TemporaryPath::create_dir("vb-cable-signature")?;
    let script = script_dir.path().join("verify-authenticode.ps1");
    fs::write(&script, SCRIPT).map_err(|e| {
        WindowsVirtualCableError::new(
            "temporaryStorageFailed",
            format!("Create Authenticode verification script: {e}"),
        )
    })?;
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(&script)
        .arg(installer)
        .output()
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "invalidSignature",
                format!("Start Authenticode verification: {e}"),
            )
        })?;
    match output.status.code() {
        Some(0) => Ok(()),
        Some(3 | 4) => Err(WindowsVirtualCableError::new(
            "unexpectedPublisher",
            "VB-CABLE setup was signed by an unexpected publisher",
        )),
        code => {
            let status = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let detail = if !status.is_empty() {
                format!("Authenticode status: {status}")
            } else if !stderr.is_empty() {
                format!("Authenticode verification failed: {stderr}")
            } else {
                format!("Authenticode verification exited with {code:?}")
            };
            Err(WindowsVirtualCableError::new("invalidSignature", detail))
        }
    }
}

fn current_install_root() -> Result<PathBuf, WindowsVirtualCableError> {
    std::env::current_exe()
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "operationFailed",
                format!("Locate Splitwave executable: {e}"),
            )
        })?
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            WindowsVirtualCableError::new(
                "operationFailed",
                "Splitwave executable has no installation directory",
            )
        })
}

fn consumer_id(root: &Path) -> Result<String, WindowsVirtualCableError> {
    let normalized = root
        .canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .to_string_lossy()
        .replace('/', "\\")
        .to_ascii_lowercase();
    let identity = format!("{}|{normalized}", current_user_sid()?);
    let hash = Sha256::digest(identity.as_bytes());
    Ok(format!(
        "splitwave-{}",
        hash.iter()
            .take(12)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    ))
}

fn supplied_consumer_id(args: &[OsString]) -> Result<String, WindowsVirtualCableError> {
    let consumer = args
        .get(4)
        .and_then(|value| value.to_str())
        .ok_or_else(|| {
            WindowsVirtualCableError::new(
                "invalidHelperArguments",
                "Missing VB-CABLE consumer identity",
            )
        })?;
    let valid = consumer.strip_prefix("splitwave-").is_some_and(|hash| {
        hash.len() == 24 && hash.chars().all(|character| character.is_ascii_hexdigit())
    });
    if !valid {
        return Err(WindowsVirtualCableError::new(
            "invalidHelperArguments",
            "Invalid VB-CABLE consumer identity",
        ));
    }
    Ok(consumer.into())
}

fn helper_consumer_id(args: &[OsString], root: &Path) -> Result<String, WindowsVirtualCableError> {
    if args.get(4).is_some() {
        supplied_consumer_id(args)
    } else {
        consumer_id(root)
    }
}

fn args_with_consumer(args: &[OsString], consumer: &str) -> Vec<OsString> {
    if args.get(4).is_some() {
        return args.to_vec();
    }
    let mut rerun_args = args.to_vec();
    rerun_args.push(OsString::from(consumer));
    rerun_args
}

fn current_user_sid() -> Result<String, WindowsVirtualCableError> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| {
        WindowsVirtualCableError::new("consumerIdentityFailed", format!("Open process token: {e}"))
    })?;
    let result = (|| {
        let mut required = 0;
        unsafe {
            let _ = GetTokenInformation(token, TokenUser, None, 0, &mut required);
        }
        if required == 0 {
            return Err(WindowsVirtualCableError::new(
                "consumerIdentityFailed",
                "Windows did not report the size of the current user token",
            ));
        }
        let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0_usize; words];
        let user = buffer.as_mut_ptr().cast::<TOKEN_USER>();
        unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                Some(user.cast::<c_void>()),
                required,
                &mut required,
            )
        }
        .map_err(|e| {
            WindowsVirtualCableError::new(
                "consumerIdentityFailed",
                format!("Read process user token: {e}"),
            )
        })?;
        let mut sid = PWSTR::null();
        unsafe { ConvertSidToStringSidW((*user).User.Sid, &mut sid) }.map_err(|e| {
            WindowsVirtualCableError::new(
                "consumerIdentityFailed",
                format!("Format process user SID: {e}"),
            )
        })?;
        let value = unsafe { sid.to_string() }.map_err(|e| {
            WindowsVirtualCableError::new(
                "consumerIdentityFailed",
                format!("Read process user SID: {e}"),
            )
        });
        unsafe {
            let _ = LocalFree(Some(HLOCAL(sid.0.cast())));
        }
        value
    })();
    unsafe {
        let _ = CloseHandle(token);
    }
    result
}

fn now_unix_seconds() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

fn helper_error(action: &str, exit_code: u32) -> WindowsVirtualCableError {
    if exit_code == ERROR_CANCELLED.0 as u32 {
        WindowsVirtualCableError::new("elevationCancelled", "Administrator approval was cancelled")
    } else {
        warn!(action, exit_code, "VB-CABLE elevated helper failed");
        WindowsVirtualCableError::new(
            "installerFailed",
            match action {
                "install" => {
                    "VB-CABLE could not be installed. No usable virtual microphone was detected."
                }
                "remove" => "VB-CABLE could not be removed. The existing driver was preserved.",
                _ => "The VB-CABLE operation could not be completed.",
            },
        )
    }
}

fn elevate_current(args: &[OsString]) -> Result<u32, WindowsVirtualCableError> {
    let executable = std::env::current_exe().map_err(|e| {
        WindowsVirtualCableError::new("operationFailed", format!("Locate Splitwave helper: {e}"))
    })?;
    let file = wide(executable.as_os_str());
    let verb = wide("runas");
    let parameters = wide(
        &args
            .iter()
            .map(|arg| quote_windows_arg(arg))
            .collect::<Vec<_>>()
            .join(" "),
    );
    let mut execute = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: PCWSTR(verb.as_ptr()),
        lpFile: PCWSTR(file.as_ptr()),
        lpParameters: PCWSTR(parameters.as_ptr()),
        nShow: 1,
        ..Default::default()
    };
    unsafe { ShellExecuteExW(&mut execute) }.map_err(|e| {
        let cancelled = (e.code().0 as u32 & 0xffff) == ERROR_CANCELLED.0;
        WindowsVirtualCableError::new(
            if cancelled {
                "elevationCancelled"
            } else {
                "elevationFailed"
            },
            "Administrator approval is required to manage VB-CABLE",
        )
    })?;
    unsafe {
        WaitForSingleObject(execute.hProcess, INFINITE);
    }
    let mut code = 1;
    unsafe { GetExitCodeProcess(execute.hProcess, &mut code) }.map_err(|e| {
        WindowsVirtualCableError::new("operationFailed", format!("Read helper exit code: {e}"))
    })?;
    unsafe {
        let _ = CloseHandle(execute.hProcess);
    }
    Ok(code)
}

fn quote_windows_arg(value: &OsString) -> String {
    let value = value.to_string_lossy();
    let mut result = String::from("\"");
    let mut slashes = 0;
    for character in value.chars() {
        match character {
            '\\' => slashes += 1,
            '\"' => {
                result.push_str(&"\\".repeat(slashes * 2 + 1));
                result.push('\"');
                slashes = 0;
            }
            _ => {
                result.push_str(&"\\".repeat(slashes));
                result.push(character);
                slashes = 0;
            }
        }
    }
    result.push_str(&"\\".repeat(slashes * 2));
    result.push('\"');
    result
}

fn wide(value: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
    value
        .as_ref()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn is_elevated() -> Result<bool, WindowsVirtualCableError> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.map_err(|e| {
        WindowsVirtualCableError::new("elevationFailed", format!("Open process token: {e}"))
    })?;
    let mut elevation = TOKEN_ELEVATION::default();
    let mut returned = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&mut elevation as *mut TOKEN_ELEVATION).cast::<c_void>()),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    };
    unsafe {
        let _ = CloseHandle(token);
    }
    result.map(|_| elevation.TokenIsElevated != 0).map_err(|e| {
        WindowsVirtualCableError::new("elevationFailed", format!("Read process token: {e}"))
    })
}
