//! Read-only IMS dump interpretation. No provisioning or network changes.
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(crate) struct ImsVoice {
    pub(crate) voice: Option<bool>,
    sms: Option<bool>,
    pub(crate) reg_state: Option<i32>,
    wlan: Option<bool>,
    technology: Option<i32>,
    event_current: Option<bool>,
    conflicting: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImsDiagnostic {
    status: &'static str,
    registration: &'static str,
    voice: Option<bool>,
    sms: Option<bool>,
    transport: &'static str,
    technology: &'static str,
}

impl ImsVoice {
    pub(crate) fn volte(&self) -> &'static str {
        if self.conflicting {
            return "unknown";
        }
        match (self.reg_state, self.voice, self.transport()) {
            (Some(0), _, _) | (_, Some(false), _) => "off",
            (Some(2), Some(true), "wifi") => "wifi",
            (Some(2), Some(true), "cellular") => "on",
            _ => "unknown",
        }
    }

    fn transport(&self) -> &'static str {
        if self.conflicting || self.event_current == Some(false) {
            return "unknown";
        }
        match self.technology {
            Some(0 | 3) => "cellular", // LTE / NR. WWAN alone does not prove LTE.
            Some(1) => "wifi",
            Some(2 | 4) => "other", // cross-SIM / IMS over 3G are not target cellular VoLTE.
            Some(_) => "unknown",
            None => match self.wlan {
                Some(true) => "wifi",
                Some(false) => "cellular",
                None => "unknown",
            },
        }
    }
}

fn capability(line: &str, name: &str) -> Option<bool> {
    let value = line
        .split_once(name)?
        .1
        .trim_start()
        .split([' ', ']', ','])
        .next()?;
    match value {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

fn current_value<T: Copy + PartialEq>(
    stored: &mut Option<T>,
    value: Option<T>,
    conflict: &mut bool,
) {
    if stored.is_some() && value.is_some() && *stored != value {
        *conflict = true;
    }
    if value.is_some() {
        *stored = value;
    }
}

fn integer_field(line: &str, name: &str) -> Option<i32> {
    line.strip_prefix(name)?
        .trim_start_matches([' ', '='])
        .trim()
        .parse()
        .ok()
}

pub(crate) fn parse_ims_voice(out: &str) -> HashMap<u8, ImsVoice> {
    let mut map: HashMap<u8, ImsVoice> = HashMap::new();
    let mut phone = None;
    for line in out.lines() {
        let t = line.trim();
        if t.starts_with("mPhoneId=") {
            // Reject malformed/large IDs rather than wrapping into another SIM slot.
            phone = integer_field(t, "mPhoneId")
                .filter(|id| (0..=1).contains(id))
                .map(|id| (id + 1) as u8);
            continue;
        }
        let Some(slot) = phone else { continue };
        let e = map.entry(slot).or_default();
        if t.starts_with("mMmTelCapabilities=") {
            current_value(&mut e.voice, capability(t, "Voice:"), &mut e.conflicting);
            current_value(&mut e.sms, capability(t, "SMS:"), &mut e.conflicting);
        } else if t.starts_with("mImsMmTelRegistrationState") {
            current_value(
                &mut e.reg_state,
                integer_field(t, "mImsMmTelRegistrationState"),
                &mut e.conflicting,
            );
        } else if t.starts_with("mImsRegistrationTech") {
            current_value(
                &mut e.technology,
                integer_field(t, "mImsRegistrationTech"),
                &mut e.conflicting,
            );
        } else if t.contains("handleImsRegistered:") {
            // A new unknown-format event must not inherit an older known transport.
            e.event_current = Some(true);
            let old = t
                .split_once("imsRadioTech=")
                .map(|(_, v)| v.split_whitespace().next().unwrap_or(""));
            let new = t
                .split_once("imsTransportType=")
                .map(|(_, v)| v.split_whitespace().next().unwrap_or(""));
            e.wlan = match (old, new) {
                (Some(a), Some(b)) if a != b => {
                    e.conflicting = true;
                    None
                }
                (_, Some("WWAN")) | (Some("WWAN"), None) => Some(false),
                (_, Some("WLAN")) | (Some("WLAN"), None) => Some(true),
                _ => None,
            };
        } else if t.contains("handleImsUnregistered:") || t.contains("handleImsRegistering:") {
            e.event_current = Some(false);
            e.wlan = None;
        }
    }
    map
}

pub(crate) fn diagnostics(state: &str, query_ok: bool, value: Option<&ImsVoice>) -> ImsDiagnostic {
    let e = value.copied().unwrap_or_default();
    let registration = match e.reg_state {
        Some(0) => "not-registered",
        Some(1) => "registering",
        Some(2) => "registered",
        _ => "unknown",
    };
    let status = match state {
        "ABSENT" => "no-sim",
        s if s != "LOADED" => "sim-not-ready",
        _ if !query_ok => "query-failed",
        _ if e.conflicting => "conflicting-evidence",
        _ if registration == "not-registered" => "not-registered",
        _ if registration == "registering" => "registering",
        _ if e.voice == Some(false) => "voice-unavailable",
        _ if e.volte() == "on" => "registered",
        _ if e.volte() == "wifi" => "wifi-only",
        _ if e.technology == Some(2) => "cross-sim",
        _ if e.technology == Some(4) => "other-network",
        _ if registration == "registered" && e.voice == Some(true) => "transport-unknown",
        _ => "unsupported-format",
    };
    // Failed queries and absent/locked SIMs expose no stale evidence.
    let usable = query_ok && state == "LOADED";
    ImsDiagnostic {
        status,
        registration: if usable { registration } else { "unknown" },
        voice: if usable { e.voice } else { None },
        sms: if usable { e.sms } else { None },
        transport: if usable { e.transport() } else { "unknown" },
        technology: if usable {
            match e.technology {
                Some(0) => "lte",
                Some(1) => "iwlan",
                Some(2) => "cross-sim",
                Some(3) => "nr",
                Some(4) => "3g",
                _ => "unknown",
            }
        } else {
            "unknown"
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dump(event: &str) -> String {
        format!("mPhoneId=0\nmMmTelCapabilities=MmTel Capabilities - [Voice: true SMS: false]\nmImsMmTelRegistrationState = 2\n{event}\n")
    }
    #[test]
    fn old_and_new_transport_names_and_slot_separation() {
        for name in ["imsRadioTech", "imsTransportType"] {
            let raw = format!("{}mPhoneId=1\nmMmTelCapabilities=MmTel Capabilities - [Voice: true SMS: true]\nmImsMmTelRegistrationState = 2\nhandleImsRegistered: {name}=WLAN", dump(&format!("handleImsRegistered: {name}=WWAN")));
            let parsed = parse_ims_voice(&raw);
            assert_eq!(parsed[&1].volte(), "on");
            assert_eq!(parsed[&2].volte(), "wifi");
            assert_eq!(diagnostics("LOADED", true, parsed.get(&1)).sms, Some(false));
        }
    }
    #[test]
    fn stale_or_unknown_new_events_do_not_inherit_old_registration() {
        for event in [
            "handleImsUnregistered: reason=0",
            "handleImsRegistering: imsRadioTech=WWAN",
            "handleImsRegistered: imsTransportType=INVALID",
        ] {
            let parsed = parse_ims_voice(&dump(&format!(
                "handleImsRegistered: imsRadioTech=WWAN\n{event}"
            )));
            assert_eq!(parsed[&1].volte(), "unknown");
        }
    }
    #[test]
    fn malformed_capabilities_and_duplicate_conflicts_never_prove_success() {
        for extra in [
            "mMmTelCapabilities=MmTel Capabilities - [Voice: false]",
            "mImsMmTelRegistrationState = 0",
            "handleImsRegistered: imsRadioTech=WWAN imsTransportType=WLAN",
        ] {
            let parsed = parse_ims_voice(&format!(
                "{}{extra}",
                dump("handleImsRegistered: imsTransportType=WWAN")
            ));
            assert_eq!(
                diagnostics("LOADED", true, parsed.get(&1)).status,
                "conflicting-evidence"
            );
        }
        let malformed = dump("handleImsRegistered: imsTransportType=WWAN")
            .replace("Voice: true", "Voice: truth");
        assert_eq!(parse_ims_voice(&malformed)[&1].volte(), "unknown");
        assert!(parse_ims_voice("mPhoneId=256\nmImsMmTelRegistrationState = 2").is_empty());
    }
    #[test]
    fn network_technology_and_query_outcomes_are_independent() {
        for (tech, status, label) in [
            (0, "registered", "lte"),
            (1, "wifi-only", "iwlan"),
            (2, "cross-sim", "cross-sim"),
            (3, "registered", "nr"),
            (4, "other-network", "3g"),
        ] {
            let parsed = parse_ims_voice(&format!("{}mImsRegistrationTech = {tech}", dump("")));
            let result = diagnostics("LOADED", true, parsed.get(&1));
            assert_eq!(result.status, status);
            assert_eq!(result.technology, label);
            assert_eq!(diagnostics("ABSENT", true, parsed.get(&1)).status, "no-sim");
            let failed = diagnostics("LOADED", false, parsed.get(&1));
            assert_eq!(failed.status, "query-failed");
            assert_eq!(failed.voice, None);
        }
        assert_eq!(
            diagnostics("PIN_REQUIRED", true, None).status,
            "sim-not-ready"
        );
        assert_eq!(
            diagnostics("LOADED", true, None).status,
            "unsupported-format"
        );
    }
}
