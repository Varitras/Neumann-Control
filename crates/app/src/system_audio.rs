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
}

pub trait SystemAudio {
    fn devices(&mut self) -> Vec<AudioDevice>;
    /// `device` `None` is Windows' default playback device; `None` back when it is gone.
    fn state(&mut self, device: Option<&str>) -> Option<AudioState>;
    fn set_volume(&mut self, device: Option<&str>, volume: f32);
    fn set_mute(&mut self, device: Option<&str>, mute: bool);
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
        eConsole, eRender, IMMDevice, IMMDeviceEnumerator, MMDeviceEnumerator, DEVICE_STATE_ACTIVE,
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
    }

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

/// Where Core Audio does not exist: no devices.
#[cfg(not(windows))]
#[derive(Default)]
pub struct CoreAudio;

#[cfg(not(windows))]
impl SystemAudio for CoreAudio {
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
}
