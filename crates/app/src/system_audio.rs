//! The Windows volume of a playback device, for the system strip: its name, volume, mute and
//! peak level per channel. Behind a trait, so the app and its tests do not depend on a sound card.

/// A playback device Windows lists as active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioDevice {
    /// The endpoint id Windows gives the device; stored in the settings.
    pub id: String,
    pub name: String,
}

/// What the strip shows of a device.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioState {
    pub name: String,
    /// 0 to 1, as Windows' own volume slider.
    pub volume: f32,
    pub mute: bool,
    /// Peak per channel, 0 to 1, since the previous reading.
    pub peaks: Vec<f32>,
    /// Which speaker position each channel is, as Windows' mix format tells it; `None` for a
    /// format without one.
    pub channel_mask: Option<u32>,
}

pub trait SystemAudio {
    /// Whether the system has the strip at all; without, it neither shows nor is offered.
    fn offered(&self) -> bool {
        true
    }
    fn devices(&mut self) -> Vec<AudioDevice>;
    /// `device` `None` is Windows' default playback device; `None` back when it is gone.
    fn state(&mut self, device: Option<&str>) -> Option<AudioState>;
    fn set_volume(&mut self, device: Option<&str>, volume: f32);
    fn set_mute(&mut self, device: Option<&str>, mute: bool);
}

/// Windows' speaker positions in the order of their bits in a channel mask (`ksmedia.h`); the
/// channels of a device come in this order.
const SPEAKER_POSITIONS: [&str; 18] = [
    "L", "R", "C", "LFE", "BL", "BR", "FLC", "FRC", "BC", "SL", "SR", "TC", "TFL", "TFC", "TFR",
    "TBL", "TBC", "TBR",
];
/// Two channels without a mask can only be stereo.
const STEREO: [&str; 2] = ["L", "R"];

/// A name per channel from the mask's bits; a channel the mask does not name keeps its number.
pub fn channel_names(mask: Option<u32>, count: usize) -> Vec<String> {
    let named: Vec<&str> = match mask {
        Some(mask) => SPEAKER_POSITIONS
            .iter()
            .enumerate()
            .filter(|(bit, _)| mask & (1 << bit) != 0)
            .map(|(_, name)| *name)
            .collect(),
        None if count == STEREO.len() => STEREO.to_vec(),
        None => Vec::new(),
    };
    (0..count)
        .map(|channel| {
            named
                .get(channel)
                .map_or_else(|| (channel + 1).to_string(), |name| (*name).to_owned())
        })
        .collect()
}

/// A peak value (0 to 1) as dB for the meters; silence is the meters' floor.
pub fn peak_db(peak: f32, floor: f64) -> f64 {
    if peak <= 0.0 {
        return floor;
    }
    (20.0 * f64::from(peak).log10()).max(floor)
}

#[cfg(windows)]
pub use core_audio::CoreAudio;

#[cfg(windows)]
mod core_audio {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
    use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioClient, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator,
        DEVICE_STATE_ACTIVE, WAVEFORMATEXTENSIBLE,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_APARTMENTTHREADED,
        STGM_READ,
    };

    use super::{AudioDevice, AudioState, SystemAudio};

    /// The device last read, so a frame does not activate its interfaces again.
    struct Endpoint {
        /// What the caller asked for (`None`: the default device).
        wanted: Option<String>,
        id: String,
        name: String,
        volume: IAudioEndpointVolume,
        meter: IAudioMeterInformation,
        channel_mask: Option<u32>,
    }

    /// `WAVE_FORMAT_EXTENSIBLE`: the format carries a channel mask.
    const WAVE_FORMAT_EXTENSIBLE: u16 = 0xFFFE;
    /// The bytes an extensible format adds to `WAVEFORMATEX`, the mask among them.
    const EXTENSIBLE_EXTRA_BYTES: u16 = 22;

    pub struct CoreAudio {
        enumerator: Option<IMMDeviceEnumerator>,
        endpoint: Option<Endpoint>,
    }

    impl CoreAudio {
        /// COM on the UI thread; when eframe initialised it already, that apartment is used.
        pub fn new() -> CoreAudio {
            // SAFETY: called on the thread that uses the interfaces; a second initialisation
            // only reports that COM is already up.
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            // SAFETY: plain COM object creation; a failure leaves the strip without a device.
            let enumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }.ok();
            CoreAudio {
                enumerator,
                endpoint: None,
            }
        }

        fn device(&self, wanted: Option<&str>) -> Option<IMMDevice> {
            let enumerator = self.enumerator.as_ref()?;
            // SAFETY: COM calls on a live enumerator.
            let device = unsafe {
                match wanted {
                    None => enumerator.GetDefaultAudioEndpoint(eRender, eConsole),
                    Some(id) => enumerator.GetDevice(&HSTRING::from(id)),
                }
            }
            .ok()?;
            // SAFETY: as above.
            let state = unsafe { device.GetState() }.ok()?;
            (state == DEVICE_STATE_ACTIVE).then_some(device)
        }

        /// The cached endpoint when it is still the one asked for, else a fresh one.
        fn endpoint(&mut self, wanted: Option<&str>) -> Option<&Endpoint> {
            let device = self.device(wanted)?;
            let id = device_id(&device)?;
            let current = self
                .endpoint
                .as_ref()
                .is_some_and(|endpoint| endpoint.wanted.as_deref() == wanted && endpoint.id == id);
            if !current {
                // SAFETY: activating interfaces of a live device.
                let volume =
                    unsafe { device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) }.ok()?;
                // SAFETY: as above.
                let meter =
                    unsafe { device.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None) }.ok()?;
                self.endpoint = Some(Endpoint {
                    wanted: wanted.map(str::to_owned),
                    name: device_name(&device).unwrap_or_else(|| id.clone()),
                    id,
                    volume,
                    meter,
                    channel_mask: channel_mask(&device),
                });
            }
            self.endpoint.as_ref()
        }
    }

    impl Default for CoreAudio {
        fn default() -> Self {
            CoreAudio::new()
        }
    }

    fn device_id(device: &IMMDevice) -> Option<String> {
        // SAFETY: GetId returns a string the caller frees with CoTaskMemFree.
        unsafe {
            let id: PWSTR = device.GetId().ok()?;
            let text = id.to_string().ok();
            CoTaskMemFree(Some(id.0 as *const _));
            text
        }
    }

    /// The mask of the shared mix format, which the meters' channels follow.
    fn channel_mask(device: &IMMDevice) -> Option<u32> {
        // SAFETY: the client is activated on a live device; GetMixFormat returns a format the
        // caller frees with CoTaskMemFree, read only while it lives (fields by value, the
        // structs are packed).
        unsafe {
            let client = device.Activate::<IAudioClient>(CLSCTX_ALL, None).ok()?;
            let format = client.GetMixFormat().ok()?;
            let tag = (*format).wFormatTag;
            let extra = (*format).cbSize;
            let extensible = tag == WAVE_FORMAT_EXTENSIBLE && extra >= EXTENSIBLE_EXTRA_BYTES;
            let mask = extensible.then(|| (*format.cast::<WAVEFORMATEXTENSIBLE>()).dwChannelMask);
            CoTaskMemFree(Some(format as *const _));
            mask
        }
    }

    fn device_name(device: &IMMDevice) -> Option<String> {
        // SAFETY: reading one property of a live device.
        unsafe {
            let store = device.OpenPropertyStore(STGM_READ).ok()?;
            let value = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
            Some(value.to_string())
        }
    }

    impl SystemAudio for CoreAudio {
        fn devices(&mut self) -> Vec<AudioDevice> {
            let Some(enumerator) = self.enumerator.as_ref() else {
                return Vec::new();
            };
            // SAFETY: COM calls on a live enumerator and the collection it returns.
            unsafe {
                let Ok(collection) = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
                else {
                    return Vec::new();
                };
                let count = collection.GetCount().unwrap_or(0);
                (0..count)
                    .filter_map(|place| collection.Item(place).ok())
                    .filter_map(|device| {
                        let id = device_id(&device)?;
                        let name = device_name(&device).unwrap_or_else(|| id.clone());
                        Some(AudioDevice { id, name })
                    })
                    .collect()
            }
        }

        fn state(&mut self, device: Option<&str>) -> Option<AudioState> {
            let endpoint = self.endpoint(device)?;
            // SAFETY: COM calls on activated interfaces; a vanished device answers an error.
            let read = unsafe {
                let volume = endpoint.volume.GetMasterVolumeLevelScalar().ok()?;
                let mute = endpoint.volume.GetMute().ok()?.as_bool();
                let channels = endpoint.meter.GetMeteringChannelCount().ok()?;
                let mut peaks = vec![0.0; channels as usize];
                endpoint.meter.GetChannelsPeakValues(&mut peaks).ok()?;
                Some((volume, mute, peaks))
            };
            let Some((volume, mute, peaks)) = read else {
                self.endpoint = None;
                return None;
            };
            Some(AudioState {
                name: endpoint.name.clone(),
                volume,
                mute,
                peaks,
                channel_mask: endpoint.channel_mask,
            })
        }

        fn set_volume(&mut self, device: Option<&str>, volume: f32) {
            if let Some(endpoint) = self.endpoint(device) {
                // SAFETY: COM call on an activated interface.
                let _ = unsafe {
                    endpoint
                        .volume
                        .SetMasterVolumeLevelScalar(volume.clamp(0.0, 1.0), std::ptr::null())
                };
            }
        }

        fn set_mute(&mut self, device: Option<&str>, mute: bool) {
            if let Some(endpoint) = self.endpoint(device) {
                // SAFETY: COM call on an activated interface.
                let _ = unsafe { endpoint.volume.SetMute(mute, std::ptr::null()) };
            }
        }
    }
}

/// Linux has no Core Audio; PipeWire and PulseAudio are not built in.
#[cfg(target_os = "linux")]
#[derive(Default)]
pub struct CoreAudio;

#[cfg(target_os = "linux")]
impl CoreAudio {
    pub fn new() -> CoreAudio {
        CoreAudio
    }
}

#[cfg(target_os = "linux")]
impl SystemAudio for CoreAudio {
    fn offered(&self) -> bool {
        false
    }
    fn devices(&mut self) -> Vec<AudioDevice> {
        Vec::new()
    }
    fn state(&mut self, _device: Option<&str>) -> Option<AudioState> {
        None
    }
    fn set_volume(&mut self, _device: Option<&str>, _volume: f32) {}
    fn set_mute(&mut self, _device: Option<&str>, _mute: bool) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_peak_reads_as_db_down_to_the_floor() {
        assert_eq!(peak_db(1.0, -120.0), 0.0);
        assert!((peak_db(0.5, -120.0) + 6.02).abs() < 0.01);
        assert_eq!(peak_db(0.0, -120.0), -120.0);
        assert_eq!(peak_db(1e-9, -60.0), -60.0);
    }

    /// Asks this machine's default playback device (run by hand: `--run-ignored only`); its
    /// mix format has a mask naming as many channels as it meters.
    #[cfg(windows)]
    #[test]
    #[ignore = "needs a sound device"]
    fn the_default_device_names_its_channels() {
        let state = CoreAudio::new()
            .state(None)
            .expect("a default playback device");
        let names = channel_names(state.channel_mask, state.peaks.len());
        eprintln!("mask {:?}: {names:?}", state.channel_mask);
        assert!(state.channel_mask.is_some());
        assert!(
            names.iter().all(|name| name.parse::<usize>().is_err()),
            "{names:?}"
        );
    }

    fn names(mask: Option<u32>, count: usize) -> Vec<String> {
        channel_names(mask, count)
    }

    /// Windows orders the channels by the mask's bits: on 7.1 the rear pair comes before the
    /// side pair, quad has no centre.
    #[test]
    fn channels_are_named_in_the_order_of_their_mask() {
        let surround_7_1 = 0x63F;
        assert_eq!(
            names(Some(surround_7_1), 8),
            ["L", "R", "C", "LFE", "BL", "BR", "SL", "SR"]
        );
        let surround_5_1 = 0x60F;
        assert_eq!(
            names(Some(surround_5_1), 6),
            ["L", "R", "C", "LFE", "SL", "SR"]
        );
        let quad = 0x33;
        assert_eq!(names(Some(quad), 4), ["L", "R", "BL", "BR"]);
    }

    /// Without a mask only stereo is sure; a channel the mask does not cover keeps its number.
    #[test]
    fn channels_without_a_mask_are_numbered() {
        assert_eq!(names(None, 2), ["L", "R"]);
        assert_eq!(names(None, 3), ["1", "2", "3"]);
        assert_eq!(names(Some(0x3), 3), ["L", "R", "3"]);
    }
}
