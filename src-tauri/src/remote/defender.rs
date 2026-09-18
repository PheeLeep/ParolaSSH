//! Microsoft Defender posture on a Windows host. Read-only: nothing here
//! changes a setting or starts a scan.
//!
//! One PowerShell round trip gathers Defender's status, its threat history and
//! any third-party antivirus registered with Security Center; the grading is
//! pure and unit-tested.

use serde::{Deserialize, Serialize};

use super::power::powershell;
use super::{CommandOutput, OsFamily};
use crate::ssh::{SshError, SshResult};

/// Definitions older than this many days are flagged.
const STALE_DEFINITIONS_DAYS: u64 = 3;
/// And older than this, flagged as a risk rather than a warning.
const OUTDATED_DEFINITIONS_DAYS: u64 = 14;
/// A quick scan older than this is flagged.
const STALE_SCAN_DAYS: u64 = 14;
/// Defender reports "never" as `UInt32.MaxValue`; anything this large is never.
const NEVER: u64 = 65_535;
/// Detections newer than this count as recent.
const RECENT_DAYS: i64 = 30;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckState {
    Good,
    Warn,
    Bad,
    Info,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderCheck {
    pub label: String,
    pub state: CheckState,
    pub value: String,
}

/// The one-word verdict at the top of the section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    /// Defender is on and nothing needs attention.
    Protected,
    /// On, but something is stale or partly off.
    Attention,
    /// Real-time protection is off, or a threat is still active.
    AtRisk,
    /// Another antivirus is in charge; Defender stands aside.
    ThirdParty,
    /// Defender is missing or its service is not running.
    Unavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    pub name: String,
    /// `yyyy-MM-dd HH:mm`, host local time.
    pub detected: Option<String>,
    /// Still present on disk, not yet cleaned or quarantined.
    pub active: bool,
    /// Whether Defender's action (quarantine, remove) succeeded.
    pub resolved: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherAntivirus {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DefenderReport {
    pub verdict: Verdict,
    pub summary: String,
    pub checks: Vec<DefenderCheck>,
    /// Newest first.
    pub detections: Vec<Detection>,
    pub recent_detections: usize,
    pub other_antivirus: Vec<OtherAntivirus>,
    pub product_version: Option<String>,
    pub note: Option<String>,
    pub command: String,
}

/// Each probe is caught on its own, so a missing cmdlet or a denied read
/// becomes a field instead of failing the whole report.
const SCRIPT: &str = "$ErrorActionPreference='Stop'; $r=@{}; \
    function D($d) { if ($d) { $d.ToString('yyyy-MM-dd HH:mm') } }; \
    try { $s=Get-MpComputerStatus; $r.status=[pscustomobject]@{mode=[string]$s.AMRunningMode;service=[bool]$s.AMServiceEnabled;antivirus=[bool]$s.AntivirusEnabled;realtime=[bool]$s.RealTimeProtectionEnabled;behavior=[bool]$s.BehaviorMonitorEnabled;downloads=[bool]$s.IoavProtectionEnabled;tamper=$s.IsTamperProtected;sigAge=[uint32]$s.AntivirusSignatureAge;sigUpdated=(D $s.AntivirusSignatureLastUpdated);sigVersion=[string]$s.AntivirusSignatureVersion;quickAge=[uint32]$s.QuickScanAge;fullAge=[uint32]$s.FullScanAge;version=[string]$s.AMProductVersion} } catch { $r.statusError=[string]$_.Exception.Message }; \
    try { $n=@{}; Get-MpThreat | ForEach-Object { $n[[string]$_.ThreatID]=@{name=[string]$_.ThreatName;active=[bool]$_.IsActive} }; \
    $r.detections=@(Get-MpThreatDetection | Sort-Object InitialDetectionTime -Descending | Select-Object -First 20 | ForEach-Object { $t=$n[[string]$_.ThreatID]; [pscustomobject]@{name=$(if ($t) { $t.name } else { [string]$_.ThreatID });detected=(D $_.InitialDetectionTime);active=$(if ($t) { $t.active } else { $false });resolved=[bool]$_.ActionSuccess;days=$(if ($_.InitialDetectionTime) { [int]((Get-Date)-$_.InitialDetectionTime).TotalDays } else { $null })} }) } catch { $r.threatsError=[string]$_.Exception.Message }; \
    try { $r.others=@(Get-CimInstance -Namespace root/SecurityCenter2 -ClassName AntiVirusProduct | ForEach-Object { [pscustomobject]@{name=[string]$_.displayName;state=[uint32]$_.productState} }) } catch { }; \
    ConvertTo-Json -Compress -Depth 4 -InputObject $r";

pub fn command(os: OsFamily) -> SshResult<String> {
    match os {
        OsFamily::Windows => Ok(powershell(SCRIPT)),
        other => Err(SshError::unsupported(format!(
            "Microsoft Defender runs on Windows; this host is {}.",
            other.label().to_lowercase()
        ))),
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Raw {
    status: Option<RawStatus>,
    status_error: Option<String>,
    #[serde(default)]
    detections: serde_json::Value,
    threats_error: Option<String>,
    #[serde(default)]
    others: serde_json::Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawStatus {
    mode: String,
    service: bool,
    antivirus: bool,
    realtime: bool,
    behavior: bool,
    downloads: bool,
    tamper: Option<bool>,
    sig_age: u64,
    sig_updated: Option<String>,
    sig_version: String,
    quick_age: u64,
    full_age: u64,
    version: String,
}

#[derive(Debug, Deserialize)]
struct RawDetection {
    name: String,
    detected: Option<String>,
    active: bool,
    resolved: bool,
    days: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct RawProduct {
    name: String,
    state: u32,
}

/// PowerShell emits a lone object where an array had one item.
fn list<T: for<'de> Deserialize<'de>>(value: serde_json::Value) -> Vec<T> {
    match value {
        serde_json::Value::Array(items) => items
            .into_iter()
            .filter_map(|item| serde_json::from_value(item).ok())
            .collect(),
        serde_json::Value::Null => Vec::new(),
        single => serde_json::from_value(single).into_iter().collect(),
    }
}

/// Security Center's `productState`: bits 12-15 are 1 when real-time is on.
fn product_enabled(state: u32) -> bool {
    (state >> 12) & 0xF == 1
}

fn check(label: &str, state: CheckState, value: impl Into<String>) -> DefenderCheck {
    DefenderCheck {
        label: label.to_string(),
        state,
        value: value.into(),
    }
}

fn switch(label: &str, on: bool, off_state: CheckState) -> DefenderCheck {
    if on {
        check(label, CheckState::Good, "On")
    } else {
        check(label, off_state, "Off")
    }
}

fn days(age: u64) -> String {
    match age {
        0 => "today".to_string(),
        1 => "1 day ago".to_string(),
        n => format!("{n} days ago"),
    }
}

pub fn parse(output: &CommandOutput, command: String) -> SshResult<DefenderReport> {
    let text = output.stdout.trim();
    if text.is_empty() {
        return Err(SshError::Io(format!(
            "PowerShell returned nothing for Defender: {}",
            output.failure_text()
        )));
    }
    let raw: Raw = serde_json::from_str(text).map_err(|error| {
        SshError::Io(format!(
            "Unexpected Defender output from PowerShell: {error}"
        ))
    })?;
    Ok(grade(raw, command))
}

/// Turn the raw readings into checks and a verdict. Pure.
fn grade(raw: Raw, command: String) -> DefenderReport {
    let others: Vec<OtherAntivirus> = list::<RawProduct>(raw.others)
        .into_iter()
        .filter(|product| {
            !product.name.contains("Windows Defender")
                && !product.name.contains("Microsoft Defender")
        })
        .map(|product| OtherAntivirus {
            name: product.name,
            enabled: product_enabled(product.state),
        })
        .collect();
    let active_other = others
        .iter()
        .find(|product| product.enabled)
        .map(|product| product.name.clone());

    let raw_detections: Vec<RawDetection> = list(raw.detections);
    let recent_detections = raw_detections
        .iter()
        .filter(|detection| detection.days.is_some_and(|days| days <= RECENT_DAYS))
        .count();
    let detections: Vec<Detection> = raw_detections
        .into_iter()
        .map(|detection| Detection {
            name: detection.name,
            detected: detection.detected,
            active: detection.active,
            resolved: detection.resolved,
        })
        .collect();
    let threats_note = raw
        .threats_error
        .map(|error| format!("Threat history could not be read: {}", first_line(&error)));

    let Some(status) = raw.status else {
        let (verdict, summary) = match &active_other {
            Some(name) => (
                Verdict::ThirdParty,
                format!("Protected by {name}; Defender is not running."),
            ),
            None => (
                Verdict::Unavailable,
                "Defender is not available on this host, and no other antivirus is registered."
                    .to_string(),
            ),
        };
        let reason = raw
            .status_error
            .as_deref()
            .map(first_line)
            .unwrap_or_default();
        let note = Some(
            if reason.contains("not recognized") || reason.contains("CommandNotFound") {
                "The Defender PowerShell module is missing. Windows Server needs the Windows Defender feature installed.".to_string()
            } else {
                format!("Get-MpComputerStatus failed: {reason}")
            },
        );
        return DefenderReport {
            verdict,
            summary,
            checks: Vec::new(),
            detections,
            recent_detections,
            other_antivirus: others,
            product_version: None,
            note,
            command,
        };
    };

    let passive = status.mode.to_ascii_lowercase().contains("passive");
    if passive || !status.service {
        let summary = match &active_other {
            Some(name) => format!(
                "Protected by {name}; Defender is in {} and stands aside.",
                mode_label(&status.mode)
            ),
            None => format!(
                "Defender is in {} and no other antivirus reports itself on.",
                mode_label(&status.mode)
            ),
        };
        let mut checks = vec![check("Mode", CheckState::Info, mode_label(&status.mode))];
        checks.push(definitions(&status));
        return DefenderReport {
            verdict: if active_other.is_some() {
                Verdict::ThirdParty
            } else {
                Verdict::AtRisk
            },
            summary,
            checks,
            detections,
            recent_detections,
            other_antivirus: others,
            product_version: Some(status.version),
            note: threats_note,
            command,
        };
    }

    let mut checks = vec![
        check("Mode", CheckState::Info, mode_label(&status.mode)),
        switch("Antivirus", status.antivirus, CheckState::Bad),
        switch("Real-time protection", status.realtime, CheckState::Bad),
        switch("Behavior monitoring", status.behavior, CheckState::Warn),
        switch(
            "Scan downloads and attachments",
            status.downloads,
            CheckState::Warn,
        ),
        match status.tamper {
            Some(on) => switch("Tamper protection", on, CheckState::Warn),
            None => check(
                "Tamper protection",
                CheckState::Info,
                "Not reported by this Windows version",
            ),
        },
        definitions(&status),
        match status.quick_age {
            age if age >= NEVER => check("Last quick scan", CheckState::Warn, "Never"),
            age if age > STALE_SCAN_DAYS => check("Last quick scan", CheckState::Warn, days(age)),
            age => check("Last quick scan", CheckState::Good, days(age)),
        },
        match status.full_age {
            age if age >= NEVER => check("Last full scan", CheckState::Info, "Never"),
            age => check("Last full scan", CheckState::Info, days(age)),
        },
    ];
    let active_threats = detections
        .iter()
        .filter(|detection| detection.active)
        .count();
    if active_threats > 0 {
        checks.push(check(
            "Active threats",
            CheckState::Bad,
            format!("{active_threats} not yet removed"),
        ));
    }

    let off = !status.antivirus || !status.realtime;
    let verdict = if off || active_threats > 0 {
        Verdict::AtRisk
    } else if checks.iter().any(|check| check.state == CheckState::Warn) {
        Verdict::Attention
    } else {
        Verdict::Protected
    };
    let summary = match verdict {
        Verdict::AtRisk if active_threats > 0 => {
            "Defender found a threat it has not removed.".to_string()
        }
        Verdict::AtRisk => "Real-time protection is off, so new files are not scanned.".to_string(),
        Verdict::Attention => "Defender is on, but some settings need a look.".to_string(),
        _ => "Defender is on and up to date.".to_string(),
    };

    DefenderReport {
        verdict,
        summary,
        checks,
        detections,
        recent_detections,
        other_antivirus: others,
        product_version: Some(status.version),
        note: threats_note,
        command,
    }
}

fn definitions(status: &RawStatus) -> DefenderCheck {
    // Defender counts whole days, so "today" can mean a timestamp from yesterday.
    let mut value = match (&status.sig_updated, status.sig_age) {
        (Some(updated), 0) => format!("Updated {updated}"),
        (Some(updated), age) => format!("Updated {updated}, {}", days(age)),
        (None, age) => format!("Updated {}", days(age)),
    };
    if !status.sig_version.is_empty() {
        value.push_str(&format!(" (version {})", status.sig_version));
    }
    let state = match status.sig_age {
        age if age > OUTDATED_DEFINITIONS_DAYS => CheckState::Bad,
        age if age > STALE_DEFINITIONS_DAYS => CheckState::Warn,
        _ => CheckState::Good,
    };
    check("Virus definitions", state, value)
}

fn mode_label(mode: &str) -> String {
    match mode {
        "Normal" => "active mode".to_string(),
        "" => "an unknown mode".to_string(),
        other => other.to_lowercase(),
    }
}

fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or_default().trim()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn out(stdout: &str) -> CommandOutput {
        CommandOutput {
            stdout: stdout.into(),
            stderr: String::new(),
            exit_code: Some(0),
        }
    }

    const HEALTHY: &str = r#"{"status":{"mode":"Normal","service":true,"antivirus":true,"realtime":true,"behavior":true,"downloads":true,"tamper":true,"sigAge":0,"sigUpdated":"2026-09-18 08:00","sigVersion":"1.417.1.0","quickAge":2,"fullAge":4294967295,"version":"4.18.24090.11"},"detections":[],"others":{"name":"Windows Defender","state":397568}}"#;

    fn state_of(report: &DefenderReport, label: &str) -> CheckState {
        report
            .checks
            .iter()
            .find(|check| check.label == label)
            .unwrap()
            .state
    }

    #[test]
    fn a_healthy_defender_is_protected() {
        let report = parse(&out(HEALTHY), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::Protected);
        assert!(
            report.other_antivirus.is_empty(),
            "Defender is not its own third party"
        );
        let definitions = report.checks.iter().find(|check| check.label == "Virus definitions").unwrap();
        assert_eq!(definitions.state, CheckState::Good);
        assert_eq!(definitions.value, "Updated 2026-09-18 08:00 (version 1.417.1.0)");
        let full = report
            .checks
            .iter()
            .find(|check| check.label == "Last full scan")
            .unwrap();
        assert_eq!(full.value, "Never");
    }

    #[test]
    fn real_time_off_is_at_risk() {
        let stdout = HEALTHY.replace(r#""realtime":true"#, r#""realtime":false"#);
        let report = parse(&out(&stdout), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::AtRisk);
        assert_eq!(state_of(&report, "Real-time protection"), CheckState::Bad);
    }

    #[test]
    fn stale_definitions_warn_then_fail() {
        let week = HEALTHY.replace(r#""sigAge":0"#, r#""sigAge":5"#);
        let report = parse(&out(&week), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::Attention);
        assert_eq!(state_of(&report, "Virus definitions"), CheckState::Warn);

        let month = HEALTHY.replace(r#""sigAge":0"#, r#""sigAge":30"#);
        let report = parse(&out(&month), String::new()).unwrap();
        assert_eq!(state_of(&report, "Virus definitions"), CheckState::Bad);
    }

    #[test]
    fn an_old_windows_without_tamper_protection_is_not_penalised() {
        let stdout = HEALTHY.replace(r#""tamper":true"#, r#""tamper":null"#);
        let report = parse(&out(&stdout), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::Protected);
        assert_eq!(state_of(&report, "Tamper protection"), CheckState::Info);
    }

    #[test]
    fn an_unremoved_threat_is_at_risk_and_counts_as_recent() {
        let stdout = HEALTHY.replace(
            r#""detections":[]"#,
            r#""detections":{"name":"Virus:DOS/EICAR_Test_File","detected":"2026-09-17 10:00","active":true,"resolved":false,"days":1}"#,
        );
        let report = parse(&out(&stdout), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::AtRisk);
        assert_eq!(report.recent_detections, 1);
        assert_eq!(report.detections[0].name, "Virus:DOS/EICAR_Test_File");
        assert_eq!(state_of(&report, "Active threats"), CheckState::Bad);
    }

    #[test]
    fn passive_defender_defers_to_the_active_third_party() {
        let stdout = r#"{"status":{"mode":"Passive Mode","service":true,"antivirus":false,"realtime":false,"behavior":false,"downloads":false,"tamper":false,"sigAge":1,"sigVersion":"1.0","quickAge":4294967295,"fullAge":4294967295,"version":"4.18"},"others":[{"name":"Windows Defender","state":393472},{"name":"ESET Security","state":266240}]}"#;
        let report = parse(&out(stdout), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::ThirdParty);
        assert!(
            report.summary.contains("ESET Security"),
            "{}",
            report.summary
        );
        assert_eq!(
            report.other_antivirus,
            vec![OtherAntivirus {
                name: "ESET Security".into(),
                enabled: true
            }]
        );
    }

    #[test]
    fn a_server_without_the_module_is_unavailable_not_an_error() {
        let stdout = r#"{"statusError":"The term 'Get-MpComputerStatus' is not recognized as the name of a cmdlet","threatsError":"The term 'Get-MpThreat' is not recognized"}"#;
        let report = parse(&out(stdout), String::new()).unwrap();
        assert_eq!(report.verdict, Verdict::Unavailable);
        assert!(report.note.unwrap().contains("Windows Defender feature"));
    }

    #[test]
    fn empty_output_is_an_error() {
        let failed = CommandOutput {
            stdout: String::new(),
            stderr: "boom".into(),
            exit_code: Some(1),
        };
        assert!(parse(&failed, String::new()).is_err());
    }

    #[test]
    fn product_state_bits() {
        assert!(product_enabled(397_568)); // 0x61100: Defender on
        assert!(!product_enabled(393_472)); // 0x60100: off
    }

    #[test]
    fn only_windows_gets_a_command() {
        assert!(command(OsFamily::Windows)
            .unwrap()
            .contains("-EncodedCommand"));
        assert!(command(OsFamily::Linux).is_err());
    }
}
