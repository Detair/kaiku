//! Audio device list curation and lookup.
//!
//! On Linux CPAL talks to ALSA, which lists every PCM it knows: the sound
//! server plugins, rate converters and up/downmix plugins, and for every card
//! a dozen variants of the same hardware (`front`, `surround40/51/71`,
//! `iec958`, `dmix`, `hw`, `plughw`, ...), all with the same description.
//! [`curate_alsa_pcms`] turns that into a short list a person can pick from.
//! It is pure (no ALSA calls) so it can be tested with fake device lists.
//!
//! Devices are identified by their CPAL device id (`alsa:sysdefault:CARD=USB`,
//! `wasapi:{endpoint}`, ...), never by their human-readable name, which is
//! not unique. [`find_selection`] still accepts the description
//! that older client versions stored as the selection.

use std::collections::HashSet;

use cpal::DeviceDirection;

/// One ALSA PCM as CPAL reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlsaPcm {
    /// PCM name, e.g. `sysdefault:CARD=USB` (the CPAL device id without the
    /// `alsa:` host prefix).
    pub pcm_id: String,
    /// First description line, e.g. `Scarlett 2i2 USB, USB Audio`.
    pub description: String,
    pub direction: DeviceDirection,
}

/// One entry of the curated device list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuratedPcm {
    /// PCM name to open (unique within the list).
    pub pcm_id: String,
    /// Label to show.
    pub label: String,
    /// ALSA description, used to resolve selections stored by older client
    /// versions (they stored the description instead of an id).
    pub description: String,
    /// The entry that stands for the system default (`default`, or the
    /// sound server PCM when `default` is missing). At most one.
    pub system_default: bool,
}

/// Sound servers that the `default` PCM may route to, in order of
/// preference: (PCM name, label, name in the `default` PCM's description).
const SOUND_SERVERS: [(&str, &str, &str); 2] = [
    ("pipewire", "PipeWire", "PipeWire"),
    ("pulse", "PulseAudio", "PulseAudio"),
];

/// Device names that say nothing beyond the card name.
const GENERIC_DEVICE_NAMES: [&str; 1] = ["USB Audio"];

const fn supports(direction: DeviceDirection, input: bool) -> bool {
    match direction {
        DeviceDirection::Input => input,
        DeviceDirection::Output => !input,
        DeviceDirection::Duplex => true,
        _ => false,
    }
}

/// `CARD=USB,DEV=0` -> (`Some("USB")`, `Some("0")`).
fn card_and_dev(args: &str) -> (Option<&str>, Option<&str>) {
    let mut card = None;
    let mut dev = None;
    for part in args.split(',') {
        if let Some(value) = part.strip_prefix("CARD=") {
            card = Some(value);
        } else if let Some(value) = part.strip_prefix("DEV=") {
            dev = Some(value);
        }
    }
    (card, dev)
}

/// `Scarlett 2i2 USB, USB Audio` -> (`Scarlett 2i2 USB`, `Some("USB Audio")`).
fn split_description(description: &str) -> (&str, Option<&str>) {
    match description.split_once(", ") {
        Some((card, device)) => (card.trim(), Some(device.trim())),
        None => (description.trim(), None),
    }
}

/// Label for a card's main (analog/USB) device: the card name, plus the
/// device name when it adds information.
fn card_label(description: &str) -> String {
    match split_description(description) {
        (card, Some(device))
            if !device.is_empty() && device != card && !GENERIC_DEVICE_NAMES.contains(&device) =>
        {
            format!("{card} ({device})")
        }
        (card, _) => card.to_string(),
    }
}

/// Label for an HDMI/DisplayPort output, or `None` when nothing is plugged
/// in: the kernel names the PCM after the connected monitor (from its EDID),
/// and leaves the generic `HDMI 0`, `HDMI 1`, ... on unused ports.
fn hdmi_label(description: &str) -> Option<String> {
    let (_card, device) = split_description(description);
    let device = device?;
    let unconnected = device
        .strip_prefix("HDMI ")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
    if unconnected || device.is_empty() {
        return None;
    }
    Some(format!("{device} (display audio)"))
}

/// Rank of a card-level PCM: lower wins. `sysdefault` is the card's own
/// default (with the distribution's sharing/conversion plugins), `front`
/// is the plain stereo device on cards whose config lacks `sysdefault`.
fn card_pcm_rank(plugin: &str, dev: Option<&str>) -> Option<u8> {
    match (plugin, dev) {
        ("sysdefault", _) => Some(0),
        ("front", None | Some("0")) => Some(1),
        _ => None,
    }
}

/// Curate the ALSA PCM list for one direction (`input` = capture).
///
/// The result is, in order:
/// 1. The system default (`default`), labelled with the sound server it routes to, e.g. "System
///    default (`PipeWire`)". If `default` does not route to a sound server but `pipewire`/`pulse`
///    exist, that server is listed right after it (only the preferred one: `pulse` on a `PipeWire`
///    system is `PipeWire` again).
/// 2. One entry per sound card for its plain device (`sysdefault:CARD=…`, else
///    `front:CARD=…,DEV=0`), labelled with the card name once.
/// 3. Outputs only: one entry per HDMI/DisplayPort device with a monitor connected, labelled with
///    the monitor name.
///
/// Everything else is dropped: channel-layout variants (`surround*`),
/// digital passthrough (`iec958`), sharing plugins (`dmix`, `dsnoop`), raw
/// hardware (`hw`, `plughw`), converters/effects (`upmix`, `vdownmix`,
/// `speex`, `*rate`), `null`, `jack`, `oss`, `usbstream`, and the
/// card-less `sysdefault`. Ids and labels in the result are unique.
pub fn curate_alsa_pcms(pcms: &[AlsaPcm], input: bool) -> Vec<CuratedPcm> {
    let pcms: Vec<&AlsaPcm> = pcms
        .iter()
        .filter(|pcm| supports(pcm.direction, input))
        .collect();

    let mut system = Vec::new();
    let default = pcms.iter().find(|pcm| pcm.pcm_id == "default");
    let default_server = default.and_then(|pcm| {
        SOUND_SERVERS
            .iter()
            .find(|(_, _, needle)| pcm.description.contains(needle))
    });
    if let Some(pcm) = default {
        let label = match default_server {
            Some((_, server, _)) => format!("System default ({server})"),
            None => "System default".to_string(),
        };
        system.push(CuratedPcm {
            pcm_id: pcm.pcm_id.clone(),
            label,
            description: pcm.description.clone(),
            system_default: true,
        });
    }
    if default_server.is_none() {
        let server = SOUND_SERVERS.iter().find_map(|(id, label, _)| {
            pcms.iter()
                .find(|pcm| pcm.pcm_id == *id)
                .map(|pcm| (pcm, label))
        });
        if let Some((pcm, label)) = server {
            let label = if default.is_some() {
                (*label).to_string()
            } else {
                format!("System default ({label})")
            };
            system.push(CuratedPcm {
                pcm_id: pcm.pcm_id.clone(),
                label,
                description: pcm.description.clone(),
                system_default: default.is_none(),
            });
        }
    }

    // Best PCM per card, in the order cards first appear.
    let mut cards: Vec<(&str, u8, &AlsaPcm)> = Vec::new();
    let mut hdmi = Vec::new();
    for pcm in &pcms {
        let Some((plugin, args)) = pcm.pcm_id.split_once(':') else {
            continue;
        };
        let (Some(card), dev) = card_and_dev(args) else {
            continue;
        };
        if plugin == "hdmi" {
            if !input {
                if let Some(label) = hdmi_label(&pcm.description) {
                    hdmi.push(CuratedPcm {
                        pcm_id: pcm.pcm_id.clone(),
                        label,
                        description: pcm.description.clone(),
                        system_default: false,
                    });
                }
            }
            continue;
        }
        let Some(rank) = card_pcm_rank(plugin, dev) else {
            continue;
        };
        match cards.iter_mut().find(|(c, _, _)| *c == card) {
            Some(entry) if rank < entry.1 => *entry = (card, rank, pcm),
            Some(_) => {}
            None => cards.push((card, rank, pcm)),
        }
    }

    let card_entries = cards.into_iter().map(|(_, _, pcm)| CuratedPcm {
        pcm_id: pcm.pcm_id.clone(),
        label: card_label(&pcm.description),
        description: pcm.description.clone(),
        system_default: false,
    });

    let mut seen_ids = HashSet::new();
    let mut seen_labels = HashSet::new();
    let mut curated = Vec::new();
    for mut entry in system.into_iter().chain(card_entries).chain(hdmi) {
        if !seen_ids.insert(entry.pcm_id.clone()) {
            continue;
        }
        // Two identical cards (e.g. two of the same headset): tell them
        // apart by the ALSA card id.
        if !seen_labels.insert(entry.label.clone()) {
            entry.label = format!("{} [{}]", entry.label, entry.pcm_id);
            seen_labels.insert(entry.label.clone());
        }
        curated.push(entry);
    }
    curated
}

/// Label of a PCM the curated list hides (shown with "Show all ALSA
/// devices"): `{card} — {detail} [{pcm}]` when ALSA has a second
/// description line (e.g. "IEC958 (S/PDIF) Digital Audio Output"), else
/// `{description} [{pcm}]`. The PCM name keeps labels unique.
pub fn raw_pcm_label(pcm_id: &str, description: &str, detail: Option<&str>) -> String {
    let detail = detail.map(str::trim).filter(|d| !d.is_empty());
    match detail {
        Some(detail) => {
            let (card, _) = split_description(description);
            format!("{card} — {detail} [{pcm_id}]")
        }
        None if description.trim().is_empty() || description == pcm_id => format!("[{pcm_id}]"),
        None => format!("{} [{pcm_id}]", description.trim()),
    }
}

/// Find the entry for a stored selection: by id first, else by the
/// human-readable name older client versions stored (first match). A
/// selection matching neither returns `None` (the caller falls back to the
/// system default or reports the device as gone).
fn position_by_id_or_legacy_name<T>(
    entries: &[T],
    selection: &str,
    id: impl Fn(&T) -> &str,
    legacy_name: impl Fn(&T) -> &str,
) -> Option<usize> {
    entries
        .iter()
        .position(|entry| id(entry) == selection)
        .or_else(|| {
            entries
                .iter()
                .position(|entry| legacy_name(entry) == selection)
        })
}

/// Where a stored selection was found; the index is into the full list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Found {
    /// One of the offered (curated) devices.
    Listed(usize),
    /// A device that exists but is not offered (filtered out by curation).
    Unlisted(usize),
}

/// Resolve a stored selection against the offered devices first (`listed`
/// holds their indices into `all`), then against every device.
///
/// The second step keeps selections working that point at a device the
/// curated list hides: an older version stored the description of e.g. an
/// `iec958` output or a custom PCM from `~/.asoundrc`, and opened that PCM.
/// Opening it again preserves what the person chose and what worked before;
/// silently switching them to the system default would not.
pub fn find_selection<T>(
    all: &[T],
    listed: &[usize],
    selection: &str,
    id: impl Fn(&T) -> &str,
    legacy_name: impl Fn(&T) -> &str,
) -> Option<Found> {
    let offered: Vec<&T> = listed.iter().map(|&i| &all[i]).collect();
    position_by_id_or_legacy_name(&offered, selection, |e| id(e), |e| legacy_name(e))
        .map(|pos| Found::Listed(listed[pos]))
        .or_else(|| {
            position_by_id_or_legacy_name(all, selection, &id, &legacy_name).map(Found::Unlisted)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(pcm_id: &str, description: &str, direction: DeviceDirection) -> AlsaPcm {
        AlsaPcm {
            pcm_id: pcm_id.to_string(),
            description: description.to_string(),
            direction,
        }
    }

    const SCARLETT: &str = "Scarlett 2i2 USB, USB Audio";

    /// What `aplay -L`/`arecord -L` (plus CPAL's own hw/plughw entries)
    /// report on a `PipeWire` desktop with a USB audio interface, an onboard
    /// codec and a GPU with three HDMI/DP ports (two monitors connected).
    fn pipewire_desktop() -> Vec<AlsaPcm> {
        use DeviceDirection::{Duplex, Output};
        vec![
            pcm(
                "null",
                "Discard all samples (playback) or generate zero samples (capture)",
                Duplex,
            ),
            pcm("sysdefault", "Default Audio Device", Duplex),
            pcm(
                "samplerate",
                "Rate Converter Plugin Using Samplerate Library",
                Duplex,
            ),
            pcm("jack", "JACK Audio Connection Kit", Duplex),
            pcm("pipewire", "PipeWire Sound Server", Duplex),
            pcm("pulse", "PulseAudio Sound Server", Duplex),
            pcm("upmix", "Plugin for channel upmix (4,6,8)", Duplex),
            pcm(
                "vdownmix",
                "Plugin for channel downmix (stereo) with a simple spacialization",
                Duplex,
            ),
            pcm(
                "default",
                "Default ALSA Output (currently PipeWire Media Server)",
                Duplex,
            ),
            pcm("sysdefault:CARD=USB", SCARLETT, Duplex),
            pcm("front:CARD=USB,DEV=0", SCARLETT, Duplex),
            pcm("surround40:CARD=USB,DEV=0", SCARLETT, Output),
            pcm("surround51:CARD=USB,DEV=0", SCARLETT, Output),
            pcm("surround71:CARD=USB,DEV=0", SCARLETT, Output),
            pcm("iec958:CARD=USB,DEV=0", SCARLETT, Output),
            pcm("dmix:CARD=USB,DEV=0", SCARLETT, Output),
            pcm("dsnoop:CARD=USB,DEV=0", SCARLETT, DeviceDirection::Input),
            pcm("usbstream:CARD=USB", "Scarlett 2i2 USB", Output),
            pcm(
                "hdmi:CARD=HDMI,DEV=0",
                "HDA ATI HDMI, LG ULTRAGEAR+",
                Output,
            ),
            pcm("hdmi:CARD=HDMI,DEV=1", "HDA ATI HDMI, AG322QWG4", Output),
            pcm("hdmi:CARD=HDMI,DEV=2", "HDA ATI HDMI, HDMI 2", Output),
            pcm(
                "sysdefault:CARD=Generic_1",
                "HD-Audio Generic, ALC897 Analog",
                Duplex,
            ),
            pcm(
                "front:CARD=Generic_1,DEV=0",
                "HD-Audio Generic, ALC897 Analog",
                Duplex,
            ),
            pcm("hw:CARD=0,DEV=0", SCARLETT, Duplex),
            pcm("plughw:CARD=0,DEV=0", SCARLETT, Duplex),
            pcm("hw:CARD=1,DEV=3", "HDA ATI HDMI, LG ULTRAGEAR+", Output),
            pcm("plughw:CARD=1,DEV=3", "HDA ATI HDMI, LG ULTRAGEAR+", Output),
        ]
    }

    fn ids_and_labels(curated: &[CuratedPcm]) -> Vec<(&str, &str)> {
        curated
            .iter()
            .map(|c| (c.pcm_id.as_str(), c.label.as_str()))
            .collect()
    }

    fn assert_unique(curated: &[CuratedPcm]) {
        let ids: HashSet<_> = curated.iter().map(|c| &c.pcm_id).collect();
        let labels: HashSet<_> = curated.iter().map(|c| &c.label).collect();
        assert_eq!(ids.len(), curated.len(), "duplicate ids: {curated:?}");
        assert_eq!(labels.len(), curated.len(), "duplicate labels: {curated:?}");
    }

    #[test]
    fn pipewire_desktop_outputs() {
        let curated = curate_alsa_pcms(&pipewire_desktop(), false);
        assert_eq!(
            ids_and_labels(&curated),
            vec![
                ("default", "System default (PipeWire)"),
                ("sysdefault:CARD=USB", "Scarlett 2i2 USB"),
                (
                    "sysdefault:CARD=Generic_1",
                    "HD-Audio Generic (ALC897 Analog)"
                ),
                ("hdmi:CARD=HDMI,DEV=0", "LG ULTRAGEAR+ (display audio)"),
                ("hdmi:CARD=HDMI,DEV=1", "AG322QWG4 (display audio)"),
            ]
        );
        assert_unique(&curated);
    }

    #[test]
    fn pipewire_desktop_inputs() {
        let curated = curate_alsa_pcms(&pipewire_desktop(), true);
        assert_eq!(
            ids_and_labels(&curated),
            vec![
                ("default", "System default (PipeWire)"),
                ("sysdefault:CARD=USB", "Scarlett 2i2 USB"),
                (
                    "sysdefault:CARD=Generic_1",
                    "HD-Audio Generic (ALC897 Analog)"
                ),
            ]
        );
        assert_unique(&curated);
    }

    #[test]
    fn front_is_used_when_card_has_no_sysdefault() {
        let pcms = vec![
            pcm(
                "front:CARD=Device,DEV=0",
                "USB PnP Sound Device, USB Audio",
                DeviceDirection::Duplex,
            ),
            pcm(
                "surround51:CARD=Device,DEV=0",
                "USB PnP Sound Device, USB Audio",
                DeviceDirection::Output,
            ),
        ];
        let curated = curate_alsa_pcms(&pcms, false);
        assert_eq!(
            ids_and_labels(&curated),
            vec![("front:CARD=Device,DEV=0", "USB PnP Sound Device")]
        );
    }

    #[test]
    fn sound_server_is_listed_when_default_does_not_route_to_it() {
        let pcms = vec![
            pcm("default", "Default Audio Device", DeviceDirection::Duplex),
            pcm("pipewire", "PipeWire Sound Server", DeviceDirection::Duplex),
            pcm("pulse", "PulseAudio Sound Server", DeviceDirection::Duplex),
        ];
        let curated = curate_alsa_pcms(&pcms, true);
        assert_eq!(
            ids_and_labels(&curated),
            vec![("default", "System default"), ("pipewire", "PipeWire")]
        );
    }

    #[test]
    fn sound_server_is_the_default_when_default_is_missing() {
        let pcms = vec![pcm(
            "pulse",
            "PulseAudio Sound Server",
            DeviceDirection::Duplex,
        )];
        let curated = curate_alsa_pcms(&pcms, false);
        assert_eq!(
            ids_and_labels(&curated),
            vec![("pulse", "System default (PulseAudio)")]
        );
    }

    #[test]
    fn identical_cards_get_distinct_labels() {
        let headset = "USB Headset, USB Audio";
        let pcms = vec![
            pcm("sysdefault:CARD=Headset", headset, DeviceDirection::Duplex),
            pcm(
                "sysdefault:CARD=Headset_1",
                headset,
                DeviceDirection::Duplex,
            ),
        ];
        let curated = curate_alsa_pcms(&pcms, true);
        assert_eq!(
            ids_and_labels(&curated),
            vec![
                ("sysdefault:CARD=Headset", "USB Headset"),
                (
                    "sysdefault:CARD=Headset_1",
                    "USB Headset [sysdefault:CARD=Headset_1]"
                ),
            ]
        );
        assert_unique(&curated);
    }

    #[test]
    fn empty_list_stays_empty() {
        assert!(curate_alsa_pcms(&[], true).is_empty());
    }

    fn find_curated<'a>(curated: &'a [CuratedPcm], selection: &str) -> Option<&'a str> {
        position_by_id_or_legacy_name(curated, selection, |c| &c.pcm_id, |c| &c.description)
            .map(|i| curated[i].pcm_id.as_str())
    }

    #[test]
    fn selection_by_id_opens_exactly_that_pcm() {
        let curated = curate_alsa_pcms(&pipewire_desktop(), false);
        assert_eq!(
            find_curated(&curated, "hdmi:CARD=HDMI,DEV=1"),
            Some("hdmi:CARD=HDMI,DEV=1")
        );
    }

    #[test]
    fn legacy_description_selection_maps_to_the_plain_card_device() {
        // Older clients stored the ALSA description; it is shared by all
        // variants of the card and must resolve to the curated entry, not
        // to e.g. `surround71`/`iec958`.
        let curated = curate_alsa_pcms(&pipewire_desktop(), false);
        assert_eq!(
            find_curated(&curated, SCARLETT),
            Some("sysdefault:CARD=USB")
        );
    }

    #[test]
    fn unknown_selection_is_not_found() {
        let curated = curate_alsa_pcms(&pipewire_desktop(), true);
        assert_eq!(find_curated(&curated, "Some Unplugged Headset"), None);
        // The label is display-only and is not accepted as a selection.
        assert_eq!(find_curated(&curated, "Scarlett 2i2 USB"), None);
    }

    /// `pipewire_desktop()` plus a custom PCM, with the indices of the
    /// curated output entries.
    fn outputs_with_custom_pcm() -> (Vec<AlsaPcm>, Vec<usize>) {
        let mut all = pipewire_desktop();
        all.push(pcm("eq", "Equalizer", DeviceDirection::Output));
        let listed = curate_alsa_pcms(&all, false)
            .iter()
            .map(|c| all.iter().position(|p| p.pcm_id == c.pcm_id).unwrap())
            .collect();
        (all, listed)
    }

    fn resolve<'a>(
        all: &'a [AlsaPcm],
        listed: &[usize],
        selection: &str,
    ) -> Option<(bool, &'a str)> {
        find_selection(all, listed, selection, |p| &p.pcm_id, |p| &p.description).map(|found| {
            match found {
                Found::Listed(i) => (true, all[i].pcm_id.as_str()),
                Found::Unlisted(i) => (false, all[i].pcm_id.as_str()),
            }
        })
    }

    #[test]
    fn selection_prefers_the_offered_entry() {
        let (all, listed) = outputs_with_custom_pcm();
        // Shared by all Scarlett variants: the curated one wins.
        assert_eq!(
            resolve(&all, &listed, SCARLETT),
            Some((true, "sysdefault:CARD=USB"))
        );
        assert_eq!(resolve(&all, &listed, "default"), Some((true, "default")));
    }

    #[test]
    fn selection_of_a_hidden_pcm_still_resolves_to_it() {
        let (all, listed) = outputs_with_custom_pcm();
        // Old description of a custom PCM, and an id of a filtered variant.
        assert_eq!(resolve(&all, &listed, "Equalizer"), Some((false, "eq")));
        assert_eq!(
            resolve(&all, &listed, "iec958:CARD=USB,DEV=0"),
            Some((false, "iec958:CARD=USB,DEV=0"))
        );
        assert_eq!(resolve(&all, &listed, "Unplugged Headset"), None);
    }

    #[test]
    fn exactly_one_system_default() {
        let curated = curate_alsa_pcms(&pipewire_desktop(), false);
        let defaults: Vec<_> = curated
            .iter()
            .filter(|c| c.system_default)
            .map(|c| c.pcm_id.as_str())
            .collect();
        assert_eq!(defaults, ["default"]);

        // `default` missing: the promoted sound server is the default.
        let pcms = vec![pcm(
            "pulse",
            "PulseAudio Sound Server",
            DeviceDirection::Duplex,
        )];
        assert!(curate_alsa_pcms(&pcms, true)[0].system_default);

        // `default` present but not routed to a server: the extra server
        // entry is not the default.
        let pcms = vec![
            pcm("default", "Default Audio Device", DeviceDirection::Duplex),
            pcm("pipewire", "PipeWire Sound Server", DeviceDirection::Duplex),
        ];
        let flags: Vec<_> = curate_alsa_pcms(&pcms, true)
            .iter()
            .map(|c| c.system_default)
            .collect();
        assert_eq!(flags, [true, false]);
    }

    #[test]
    fn raw_pcm_labels() {
        assert_eq!(
            raw_pcm_label(
                "iec958:CARD=USB,DEV=0",
                SCARLETT,
                Some("IEC958 (S/PDIF) Digital Audio Output")
            ),
            "Scarlett 2i2 USB — IEC958 (S/PDIF) Digital Audio Output [iec958:CARD=USB,DEV=0]"
        );
        assert_eq!(raw_pcm_label("eq", "Equalizer", None), "Equalizer [eq]");
        assert_eq!(
            raw_pcm_label("eq", "Equalizer", Some("  ")),
            "Equalizer [eq]"
        );
        assert_eq!(raw_pcm_label("odd", "odd", None), "[odd]");
    }

    #[test]
    fn id_match_wins_over_legacy_name() {
        let entries = [("a", "b"), ("b", "c")];
        let found = position_by_id_or_legacy_name(&entries, "b", |e| e.0, |e| e.1);
        assert_eq!(found, Some(1));
    }
}
