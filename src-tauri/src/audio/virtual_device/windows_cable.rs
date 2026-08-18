//! Managed integration for the single fixed VB-Audio VB-CABLE pair on Windows.
//!
//! This is deliberately separate from `VirtualDeviceConfig`: VB-CABLE cannot
//! implement Splitwave's arbitrary named-device semantics.

use serde::{Deserialize, Serialize};

const MANIFEST_SCHEMA_VERSION: u32 = 1;
const PROVIDER_NAME: &str = "VB-Audio VB-CABLE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowsVirtualCableState {
    NotInstalled,
    InstalledExternal,
    InstalledManaged,
    Partial,
    RebootRequired,
    RemovalPendingReboot,
    UnknownOwnership,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WindowsVirtualCableOwnership {
    External,
    Managed,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsVirtualCableStatus {
    pub state: WindowsVirtualCableState,
    pub usable: bool,
    pub provider: String,
    pub installed_version: Option<String>,
    pub render_endpoint_name: Option<String>,
    pub capture_endpoint_name: Option<String>,
    pub ownership: WindowsVirtualCableOwnership,
    pub managed_by_splitwave: bool,
    pub reboot_required: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsVirtualCableError {
    pub code: &'static str,
    pub message: String,
}

impl WindowsVirtualCableError {
    pub fn operation_failed(message: impl Into<String>) -> Self {
        Self::new("operationFailed", message)
    }

    pub fn confirmation_required() -> Self {
        Self::new(
            "confirmationRequired",
            "Confirm removal of the managed VB-CABLE driver first",
        )
    }

    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for WindowsVirtualCableError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for WindowsVirtualCableError {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CablePackage {
    provider: String,
    version: Option<String>,
    published_name: String,
    original_name: Option<String>,
    device_instance_ids: Vec<String>,
}

impl CablePackage {
    fn fingerprint(&self) -> String {
        let mut parts = self.device_instance_ids.clone();
        parts.sort();
        format!(
            "{}|{}|{}|{}",
            self.provider,
            self.version.as_deref().unwrap_or_default(),
            self.published_name,
            parts.join(",")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CableEndpointFlow {
    Render,
    Capture,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CableEndpoint {
    flow: CableEndpointFlow,
    name: String,
    parent_instance_id: String,
}

#[derive(Debug, Clone, Default)]
struct DetectedCable {
    render_endpoint_name: Option<String>,
    capture_endpoint_name: Option<String>,
    packages: Vec<CablePackage>,
}

impl DetectedCable {
    fn usable(&self) -> bool {
        self.render_endpoint_name.is_some() && self.capture_endpoint_name.is_some()
    }

    fn package(&self) -> Option<&CablePackage> {
        (self.packages.len() == 1).then(|| &self.packages[0])
    }
}

fn detect_cable_from_inventory(
    packages: Vec<CablePackage>,
    endpoints: impl IntoIterator<Item = CableEndpoint>,
) -> DetectedCable {
    let mut detected = DetectedCable {
        packages,
        ..Default::default()
    };
    for endpoint in endpoints {
        let belongs_to_cable = detected.packages.iter().any(|package| {
            package
                .device_instance_ids
                .iter()
                .any(|id| id.eq_ignore_ascii_case(&endpoint.parent_instance_id))
        });
        if !belongs_to_cable {
            continue;
        }
        match endpoint.flow {
            CableEndpointFlow::Render if detected.render_endpoint_name.is_none() => {
                detected.render_endpoint_name = Some(endpoint.name);
            }
            CableEndpointFlow::Capture if detected.capture_endpoint_name.is_none() => {
                detected.capture_endpoint_name = Some(endpoint.name);
            }
            _ => {}
        }
    }
    detected
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnershipManifest {
    schema_version: u32,
    provider: String,
    ownership: WindowsVirtualCableOwnership,
    installed_at: String,
    installed_version: Option<String>,
    installer_source: String,
    installer_sha256: String,
    installer_publisher: String,
    package_published_names: Vec<String>,
    package_original_names: Vec<String>,
    package_fingerprint: String,
    device_instance_ids: Vec<String>,
    consumer_installation_ids: Vec<String>,
    pending_reboot: bool,
    #[serde(default)]
    removal_pending_reboot: bool,
}

impl OwnershipManifest {
    fn matches(&self, package: &CablePackage) -> bool {
        self.schema_version == MANIFEST_SCHEMA_VERSION
            && self.ownership == WindowsVirtualCableOwnership::Managed
            && self.provider == PROVIDER_NAME
            && self.package_published_names.len() == 1
            && self.package_published_names[0].eq_ignore_ascii_case(&package.published_name)
            && self.package_fingerprint == package.fingerprint()
    }
}

#[derive(Debug, Clone)]
enum ManifestState {
    Missing,
    Valid(OwnershipManifest),
    Corrupt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UninstallReason {
    UserRemoval,
    Upgrade,
    Repair,
    SilentRemoval,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RemovalAction {
    Preserve,
    ReleaseConsumer,
    RetainAsExternal,
    RemoveExactPackage,
}

fn determine_state(
    detected: &DetectedCable,
    manifest: &ManifestState,
    current_consumer: &str,
) -> WindowsVirtualCableState {
    if let ManifestState::Valid(m) = manifest {
        if m.removal_pending_reboot && (!detected.packages.is_empty() || detected.usable()) {
            return WindowsVirtualCableState::RemovalPendingReboot;
        }
    }
    let Some(package) = detected.package() else {
        return if detected.packages.is_empty() && !detected.usable() {
            WindowsVirtualCableState::NotInstalled
        } else if detected.usable() {
            WindowsVirtualCableState::UnknownOwnership
        } else {
            WindowsVirtualCableState::Partial
        };
    };

    if !detected.usable() {
        return match manifest {
            ManifestState::Valid(m) if m.matches(package) && m.pending_reboot => {
                WindowsVirtualCableState::RebootRequired
            }
            _ => WindowsVirtualCableState::Partial,
        };
    }

    match manifest {
        ManifestState::Missing => WindowsVirtualCableState::InstalledExternal,
        ManifestState::Corrupt => WindowsVirtualCableState::UnknownOwnership,
        ManifestState::Valid(m) if m.ownership == WindowsVirtualCableOwnership::External => {
            WindowsVirtualCableState::InstalledExternal
        }
        ManifestState::Valid(m)
            if m.matches(package)
                && m.consumer_installation_ids
                    .iter()
                    .any(|id| id == current_consumer) =>
        {
            WindowsVirtualCableState::InstalledManaged
        }
        ManifestState::Valid(m) if m.matches(package) => WindowsVirtualCableState::InstalledManaged,
        ManifestState::Valid(_) => WindowsVirtualCableState::UnknownOwnership,
    }
}

fn removal_decision(
    state: WindowsVirtualCableState,
    manifest: &ManifestState,
    current_consumer: &str,
    reason: UninstallReason,
    user_choice: Option<bool>,
) -> RemovalAction {
    if matches!(
        reason,
        UninstallReason::Upgrade
            | UninstallReason::Repair
            | UninstallReason::SilentRemoval
            | UninstallReason::Unknown
    ) {
        return RemovalAction::Preserve;
    }
    let ManifestState::Valid(manifest) = manifest else {
        return RemovalAction::Preserve;
    };
    if state != WindowsVirtualCableState::InstalledManaged
        || manifest.ownership != WindowsVirtualCableOwnership::Managed
        || !manifest
            .consumer_installation_ids
            .iter()
            .any(|id| id == current_consumer)
    {
        return RemovalAction::Preserve;
    }
    if manifest.consumer_installation_ids.len() > 1 {
        return RemovalAction::ReleaseConsumer;
    }
    match user_choice {
        Some(true) => RemovalAction::RemoveExactPackage,
        Some(false) => RemovalAction::RetainAsExternal,
        None => RemovalAction::Preserve,
    }
}

fn status_from(
    detected: &DetectedCable,
    manifest: &ManifestState,
    current_consumer: &str,
) -> WindowsVirtualCableStatus {
    let state = determine_state(detected, manifest, current_consumer);
    let package = detected.package();
    let ownership = match state {
        WindowsVirtualCableState::InstalledExternal => WindowsVirtualCableOwnership::External,
        WindowsVirtualCableState::InstalledManaged | WindowsVirtualCableState::RebootRequired => {
            WindowsVirtualCableOwnership::Managed
        }
        WindowsVirtualCableState::UnknownOwnership => WindowsVirtualCableOwnership::Unknown,
        _ => manifest_ownership(manifest),
    };
    let reboot_required = matches!(
        state,
        WindowsVirtualCableState::RebootRequired | WindowsVirtualCableState::RemovalPendingReboot
    );
    let detail = match state {
        WindowsVirtualCableState::NotInstalled => None,
        WindowsVirtualCableState::Partial => {
            Some("VB-CABLE was detected, but Windows has not exposed both endpoints".into())
        }
        WindowsVirtualCableState::RebootRequired => Some(
            "Installation completed; restart Windows before using the virtual microphone".into(),
        ),
        WindowsVirtualCableState::RemovalPendingReboot => {
            Some("Driver removal is pending a Windows restart".into())
        }
        WindowsVirtualCableState::UnknownOwnership => {
            Some("Splitwave cannot safely determine who manages this VB-CABLE installation".into())
        }
        _ => None,
    };
    WindowsVirtualCableStatus {
        state,
        usable: detected.usable(),
        provider: package
            .map(|p| p.provider.clone())
            .unwrap_or_else(|| PROVIDER_NAME.into()),
        installed_version: package.and_then(|p| p.version.clone()),
        render_endpoint_name: detected.render_endpoint_name.clone(),
        capture_endpoint_name: detected.capture_endpoint_name.clone(),
        ownership,
        managed_by_splitwave: ownership == WindowsVirtualCableOwnership::Managed,
        reboot_required,
        detail,
    }
}

fn manifest_ownership(manifest: &ManifestState) -> WindowsVirtualCableOwnership {
    match manifest {
        ManifestState::Valid(m) => m.ownership,
        ManifestState::Corrupt => WindowsVirtualCableOwnership::Unknown,
        ManifestState::Missing => WindowsVirtualCableOwnership::External,
    }
}

fn newly_installed_package<'a>(
    before: &DetectedCable,
    after: &'a DetectedCable,
) -> Option<&'a CablePackage> {
    if before.usable() || !before.packages.is_empty() {
        return None;
    }

    let mut packages = after.packages.iter().filter(|candidate| {
        !before.packages.iter().any(|old| {
            old.published_name
                .eq_ignore_ascii_case(&candidate.published_name)
        })
    });
    let package = packages.next()?;
    packages.next().is_none().then_some(package)
}

fn verified_package_after_setup<'a>(
    before: &DetectedCable,
    after: &'a DetectedCable,
    setup_exit_code: Option<i32>,
) -> Result<&'a CablePackage, WindowsVirtualCableError> {
    newly_installed_package(before, after).ok_or_else(|| match setup_exit_code {
        Some(0) => WindowsVirtualCableError::new(
            "driverPackageNotFound",
            "VB-CABLE setup completed, but Splitwave could not identify one new exact driver package",
        ),
        Some(code) => WindowsVirtualCableError::new(
            "installerFailed",
            format!("VB-CABLE setup did not install the driver (exit code {code})"),
        ),
        None => WindowsVirtualCableError::new(
            "installerFailed",
            "VB-CABLE setup ended before Splitwave could verify the driver installation",
        ),
    })
}

#[cfg(target_os = "windows")]
mod platform;

#[cfg(target_os = "windows")]
pub use platform::{install, status};

#[cfg(target_os = "windows")]
pub fn run_helper() -> Option<i32> {
    platform::run_helper()
}

#[cfg(not(target_os = "windows"))]
pub fn status() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError> {
    Err(WindowsVirtualCableError::new(
        "unsupportedPlatform",
        "VB-CABLE integration is available only on Windows",
    ))
}

#[cfg(not(target_os = "windows"))]
pub fn install() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError> {
    status()
}

#[cfg(not(target_os = "windows"))]
pub fn run_helper() -> Option<i32> {
    None
}

pub fn windows_virtual_cable_status() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError>
{
    status()
}

pub fn install_windows_virtual_cable() -> Result<WindowsVirtualCableStatus, WindowsVirtualCableError>
{
    install()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> CablePackage {
        CablePackage {
            provider: PROVIDER_NAME.into(),
            version: Some("1.0.0".into()),
            published_name: "oem42.inf".into(),
            original_name: Some("vbMmeCable64_win10.inf".into()),
            device_instance_ids: vec!["ROOT\\VBCABLE\\0000".into()],
        }
    }

    fn detected() -> DetectedCable {
        DetectedCable {
            render_endpoint_name: Some("CABLE Input (VB-Audio Virtual Cable)".into()),
            capture_endpoint_name: Some("CABLE Output (VB-Audio Virtual Cable)".into()),
            packages: vec![package()],
        }
    }

    fn endpoint(flow: CableEndpointFlow, name: &str, parent_instance_id: &str) -> CableEndpoint {
        CableEndpoint {
            flow,
            name: name.into(),
            parent_instance_id: parent_instance_id.into(),
        }
    }

    fn managed(consumers: &[&str]) -> ManifestState {
        let package = package();
        ManifestState::Valid(OwnershipManifest {
            schema_version: MANIFEST_SCHEMA_VERSION,
            provider: PROVIDER_NAME.into(),
            ownership: WindowsVirtualCableOwnership::Managed,
            installed_at: "2026-08-17T00:00:00Z".into(),
            installed_version: package.version.clone(),
            installer_source:
                "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip".into(),
            installer_sha256: "hash".into(),
            installer_publisher: "BUREL VINCENT Entrepreneur individuel".into(),
            package_published_names: vec![package.published_name.clone()],
            package_original_names: vec![package.original_name.clone().unwrap()],
            package_fingerprint: package.fingerprint(),
            device_instance_ids: package.device_instance_ids.clone(),
            consumer_installation_ids: consumers.iter().map(ToString::to_string).collect(),
            pending_reboot: false,
            removal_pending_reboot: false,
        })
    }

    #[test]
    fn absent_cable_is_not_installed() {
        assert_eq!(
            determine_state(&DetectedCable::default(), &ManifestState::Missing, "A"),
            WindowsVirtualCableState::NotInstalled
        );
    }

    #[test]
    fn preexisting_cable_is_external() {
        assert_eq!(
            determine_state(&detected(), &ManifestState::Missing, "A"),
            WindowsVirtualCableState::InstalledExternal
        );
    }

    #[test]
    fn external_record_is_never_claimed_as_managed() {
        let ManifestState::Valid(mut manifest) = managed(&["A"]) else {
            unreachable!()
        };
        manifest.ownership = WindowsVirtualCableOwnership::External;
        assert_eq!(
            determine_state(&detected(), &ManifestState::Valid(manifest), "A"),
            WindowsVirtualCableState::InstalledExternal
        );
    }

    #[test]
    fn corrupt_or_unknown_manifest_is_not_safe_to_remove() {
        assert_eq!(
            determine_state(&detected(), &ManifestState::Corrupt, "A"),
            WindowsVirtualCableState::UnknownOwnership
        );
        let ManifestState::Valid(mut manifest) = managed(&["A"]) else {
            unreachable!()
        };
        manifest.schema_version += 1;
        assert_eq!(
            determine_state(&detected(), &ManifestState::Valid(manifest), "A"),
            WindowsVirtualCableState::UnknownOwnership
        );
    }

    #[test]
    fn managed_cable_requires_matching_package() {
        assert_eq!(
            determine_state(&detected(), &managed(&["A"]), "A"),
            WindowsVirtualCableState::InstalledManaged
        );
        let mut changed = detected();
        changed.packages[0].published_name = "oem99.inf".into();
        assert_eq!(
            determine_state(&changed, &managed(&["A"]), "A"),
            WindowsVirtualCableState::UnknownOwnership
        );
    }

    #[test]
    fn single_endpoint_is_partial() {
        let mut only_render = detected();
        only_render.capture_endpoint_name = None;
        assert_eq!(
            determine_state(&only_render, &ManifestState::Missing, "A"),
            WindowsVirtualCableState::Partial
        );
    }

    #[test]
    fn endpoint_name_without_matching_pnp_parent_is_not_a_cable() {
        let cable = detect_cable_from_inventory(
            vec![package()],
            [
                endpoint(
                    CableEndpointFlow::Render,
                    "CABLE Input (VB-Audio Virtual Cable)",
                    "ROOT\\OTHER\\0000",
                ),
                endpoint(
                    CableEndpointFlow::Capture,
                    "CABLE Output (VB-Audio Virtual Cable)",
                    "ROOT\\OTHER\\0000",
                ),
            ],
        );

        assert_eq!(
            determine_state(&cable, &ManifestState::Missing, "A"),
            WindowsVirtualCableState::Partial
        );
        assert!(!cable.usable());
    }

    #[test]
    fn matched_pnp_parent_identifies_both_cable_endpoints() {
        let cable = detect_cable_from_inventory(
            vec![package()],
            [
                endpoint(
                    CableEndpointFlow::Render,
                    "CABLE Input (VB-Audio Virtual Cable)",
                    "root\\vbcable\\0000",
                ),
                endpoint(
                    CableEndpointFlow::Capture,
                    "CABLE Output (VB-Audio Virtual Cable)",
                    "ROOT\\VBCABLE\\0000",
                ),
            ],
        );

        assert!(cable.usable());
        assert_eq!(
            cable.render_endpoint_name.as_deref(),
            Some("CABLE Input (VB-Audio Virtual Cable)")
        );
        assert_eq!(
            cable.capture_endpoint_name.as_deref(),
            Some("CABLE Output (VB-Audio Virtual Cable)")
        );
    }

    #[test]
    fn reboot_pending_package_is_not_ready() {
        let mut cable = detected();
        cable.capture_endpoint_name = None;
        let ManifestState::Valid(mut manifest) = managed(&["A"]) else {
            unreachable!()
        };
        manifest.pending_reboot = true;
        assert_eq!(
            determine_state(&cable, &ManifestState::Valid(manifest), "A"),
            WindowsVirtualCableState::RebootRequired
        );
    }

    #[test]
    fn pending_removal_is_reported_until_windows_restarts() {
        let ManifestState::Valid(mut manifest) = managed(&["A"]) else {
            unreachable!()
        };
        manifest.removal_pending_reboot = true;
        assert_eq!(
            determine_state(&detected(), &ManifestState::Valid(manifest), "A"),
            WindowsVirtualCableState::RemovalPendingReboot
        );
    }

    #[test]
    fn external_and_unknown_ownership_are_never_removed() {
        assert_eq!(
            removal_decision(
                WindowsVirtualCableState::InstalledExternal,
                &ManifestState::Missing,
                "A",
                UninstallReason::UserRemoval,
                Some(true)
            ),
            RemovalAction::Preserve
        );
        assert_eq!(
            removal_decision(
                WindowsVirtualCableState::UnknownOwnership,
                &ManifestState::Corrupt,
                "A",
                UninstallReason::UserRemoval,
                Some(true)
            ),
            RemovalAction::Preserve
        );
    }

    #[test]
    fn multiple_consumers_release_without_removing_driver() {
        assert_eq!(
            removal_decision(
                WindowsVirtualCableState::InstalledManaged,
                &managed(&["A", "B"]),
                "A",
                UninstallReason::UserRemoval,
                Some(true)
            ),
            RemovalAction::ReleaseConsumer
        );
    }

    #[test]
    fn last_consumer_default_keeps_driver_external() {
        assert_eq!(
            removal_decision(
                WindowsVirtualCableState::InstalledManaged,
                &managed(&["A"]),
                "A",
                UninstallReason::UserRemoval,
                Some(false)
            ),
            RemovalAction::RetainAsExternal
        );
    }

    #[test]
    fn retained_cable_is_external_for_future_installs() {
        let ManifestState::Valid(mut manifest) = managed(&["A"]) else {
            unreachable!()
        };
        manifest.ownership = WindowsVirtualCableOwnership::External;
        manifest.consumer_installation_ids.clear();
        assert_eq!(
            determine_state(&detected(), &ManifestState::Valid(manifest), "B"),
            WindowsVirtualCableState::InstalledExternal
        );
    }

    #[test]
    fn last_consumer_can_remove_only_after_yes() {
        assert_eq!(
            removal_decision(
                WindowsVirtualCableState::InstalledManaged,
                &managed(&["A"]),
                "A",
                UninstallReason::UserRemoval,
                Some(true)
            ),
            RemovalAction::RemoveExactPackage
        );
    }

    #[test]
    fn missing_exact_package_is_never_broadly_removed() {
        assert_eq!(
            removal_decision(
                WindowsVirtualCableState::NotInstalled,
                &managed(&["A"]),
                "A",
                UninstallReason::UserRemoval,
                Some(true)
            ),
            RemovalAction::Preserve
        );
    }

    #[test]
    fn existing_cable_is_never_considered_a_new_managed_package() {
        let package = package();
        let before = DetectedCable {
            render_endpoint_name: Some("CABLE Input".into()),
            capture_endpoint_name: Some("CABLE Output".into()),
            packages: vec![package],
        };

        assert!(newly_installed_package(&before, &before).is_none());
    }

    #[test]
    fn nonzero_setup_exit_is_accepted_after_exact_package_installation() {
        let before = DetectedCable::default();
        let after = detected();
        assert_eq!(
            verified_package_after_setup(&before, &after, Some(1))
                .expect("the exact new package proves installation")
                .published_name,
            "oem42.inf"
        );
    }

    #[test]
    fn nonzero_setup_exit_without_new_package_is_an_installer_failure() {
        let before = DetectedCable::default();
        let error = verified_package_after_setup(&before, &DetectedCable::default(), Some(1))
            .expect_err("no package was installed");
        assert_eq!(error.code, "installerFailed");
    }

    #[test]
    fn upgrade_repair_silent_and_unknown_preserve_driver() {
        for reason in [
            UninstallReason::Upgrade,
            UninstallReason::Repair,
            UninstallReason::SilentRemoval,
            UninstallReason::Unknown,
        ] {
            assert_eq!(
                removal_decision(
                    WindowsVirtualCableState::InstalledManaged,
                    &managed(&["A"]),
                    "A",
                    reason,
                    Some(true)
                ),
                RemovalAction::Preserve
            );
        }
    }
}
