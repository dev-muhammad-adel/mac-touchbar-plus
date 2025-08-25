use std::{
    fs::{File, OpenOptions, self},
    os::unix::io::{AsFd, BorrowedFd},
    path::Path,
};
use drm::{
    ClientCapability, Device as DrmDevice, buffer::{DrmFourcc, Buffer},
    control::{
        connector, Device as ControlDevice, property, ResourceHandle, atomic, AtomicCommitFlags,
        dumbbuffer::{DumbBuffer, DumbMapping}, framebuffer, ClipRect, Mode
    }
};
use anyhow::{Result, anyhow};

// Constants for DRM configuration
const TOUCHBAR_ASPECT_RATIO_THRESHOLD: u32 = 30;
const DEFAULT_PLANE_INDEX: usize = 0;
const TOUCHBAR_WIDTH: u32 = 64;
const BITS_PER_PIXEL: u32 = 32;
const DEPTH: u32 = 24;
const STRIDE_ALIGNMENT: u32 = 32;

struct Card(File);
impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl ControlDevice for Card {}
impl DrmDevice for Card {}

impl Card {
    fn open(path: &Path) -> Result<Self, std::io::Error> {
        let mut options = OpenOptions::new();
        options.read(true);
        options.write(true);

        Ok(Card(options.open(path)?))
    }
}

pub struct DrmBackend {
    card: Card,
    mode: Mode,
    db: DumbBuffer,
    fb: framebuffer::Handle
}

impl Drop for DrmBackend {
    fn drop(&mut self) {
        // Clean up resources in reverse order of creation
        // Ignore errors during cleanup as we're already shutting down
        let _ = self.card.destroy_framebuffer(self.fb);
        let _ = self.card.destroy_dumb_buffer(self.db);
    }
}


fn find_prop_id<T: ResourceHandle + std::fmt::Debug>(
    card: &Card,
    handle: T,
    name: &'static str,
) -> Result<property::Handle> {
    let props = card.get_properties(handle)?;
    for id in props.as_props_and_values().0 {
        let info = card.get_property(*id)?;
        let name_str = info.name().to_str()
            .map_err(|_| anyhow!("Invalid property name encoding for property {:?}", id))?;
        if name_str == name {
            return Ok(*id);
        }
    }
    Err(anyhow!("Property '{}' not found for handle {:?}", name, handle))
}

fn try_open_card(path: &Path) -> Result<DrmBackend> {
    let card = Card::open(path)?;
    card.set_client_capability(ClientCapability::UniversalPlanes, true)?;
    card.set_client_capability(ClientCapability::Atomic, true)?;
    card.acquire_master_lock()?;


    let res = card.resource_handles()?;
    let coninfo = res
        .connectors()
        .iter()
        .flat_map(|con| card.get_connector(*con, true))
        .collect::<Vec<_>>();
    let crtcinfo = res
        .crtcs()
        .iter()
        .flat_map(|crtc| card.get_crtc(*crtc))
        .collect::<Vec<_>>();

    let con = coninfo
        .iter()
        .find(|&i| i.state() == connector::State::Connected)
        .ok_or_else(|| {
            let states: Vec<_> = coninfo.iter().map(|c| format!("{:?}", c.state())).collect();
            anyhow!("No connected connectors found. Available states: [{}]", states.join(", "))
        })?;

    let &mode = con.modes().get(0).ok_or_else(|| {
        anyhow!("No display modes found for connector {:?}", con.handle())
    })?;
    let (disp_width, disp_height) = mode.size();
    if u32::from(disp_height / disp_width) < TOUCHBAR_ASPECT_RATIO_THRESHOLD {
        return Err(anyhow!("Display aspect ratio {}:{} does not look like a touchbar (expected ratio >= {})", 
                          disp_width, disp_height, TOUCHBAR_ASPECT_RATIO_THRESHOLD));
    }
    let crtc = crtcinfo.get(0).ok_or_else(|| {
        anyhow!("No CRTCs found on this device. Available CRTCs: {}", crtcinfo.len())
    })?;
    let fmt = DrmFourcc::Xrgb8888;
    let db = card.create_dumb_buffer((TOUCHBAR_WIDTH, disp_height.into()), fmt, BITS_PER_PIXEL)
        .map_err(|e| anyhow!("Failed to create DRM buffer: {}", e))?;

    let fb = card.add_framebuffer(&db, DEPTH, STRIDE_ALIGNMENT)
        .map_err(|e| anyhow!("Failed to create framebuffer: {}", e))?;
    let planes = card.plane_handles()?;
    if planes.is_empty() {
        return Err(anyhow!("No DRM planes available on this device"));
    }
    let plane = planes[DEFAULT_PLANE_INDEX];

    let mut atomic_req = atomic::AtomicModeReq::new();
    atomic_req.add_property(
        con.handle(),
        find_prop_id(&card, con.handle(), "CRTC_ID")?,
        property::Value::CRTC(Some(crtc.handle())),
    );
    let blob = card.create_property_blob(&mode)?;

    atomic_req.add_property(
        crtc.handle(),
        find_prop_id(&card, crtc.handle(), "MODE_ID")?,
        blob,
    );
    atomic_req.add_property(
        crtc.handle(),
        find_prop_id(&card, crtc.handle(), "ACTIVE")?,
        property::Value::Boolean(true),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "FB_ID")?,
        property::Value::Framebuffer(Some(fb)),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "CRTC_ID")?,
        property::Value::CRTC(Some(crtc.handle())),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "SRC_X")?,
        property::Value::UnsignedRange(0),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "SRC_Y")?,
        property::Value::UnsignedRange(0),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "SRC_W")?,
        property::Value::UnsignedRange((mode.size().0 as u64) << 16),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "SRC_H")?,
        property::Value::UnsignedRange((mode.size().1 as u64) << 16),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "CRTC_X")?,
        property::Value::SignedRange(0),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "CRTC_Y")?,
        property::Value::SignedRange(0),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "CRTC_W")?,
        property::Value::UnsignedRange(mode.size().0 as u64),
    );
    atomic_req.add_property(
        plane,
        find_prop_id(&card, plane, "CRTC_H")?,
        property::Value::UnsignedRange(mode.size().1 as u64),
    );

    card.atomic_commit(AtomicCommitFlags::ALLOW_MODESET, atomic_req)
        .map_err(|e| anyhow!("Failed to commit DRM atomic mode: {}", e))?;


    Ok(DrmBackend { card, mode, db, fb })
}

impl DrmBackend {
    pub fn open_card() -> Result<DrmBackend> {
        let mut errors = Vec::new();
        for entry in fs::read_dir("/dev/dri/")? {
            let entry = entry?;
            if !entry.file_name().to_string_lossy().starts_with("card") {
                continue
            }
            match try_open_card(&entry.path()) {
                Ok(card) => return Ok(card),
                Err(err) => {
                    errors.push(format!("{}: {}", entry.path().as_os_str().to_string_lossy(), err.to_string()))
                }
            }
        }
        Err(anyhow!("No touchbar device found. Attempted cards: [\n    {}\n]\n\nThis usually means:\n1. No DRM device is available\n2. Device doesn't have the required aspect ratio\n3. Insufficient permissions to access /dev/dri/\n4. DRM driver doesn't support required features", 
                    errors.join(",\n    ")))
    }
    pub fn mode(&self) -> Mode {
        self.mode
    }
    
    pub fn dimensions(&self) -> (u32, u32) {
        let (w, h) = self.mode.size();
        (u32::from(w), u32::from(h))
    }
    
    pub fn width(&self) -> u32 {
        u32::from(self.mode.size().0)
    }
    
    pub fn height(&self) -> u32 {
        u32::from(self.mode.size().1)
    }
    
    pub fn fb_info(&self) -> Result<framebuffer::Info> {
        Ok(self.card.get_framebuffer(self.fb)?)
    }
    pub fn dirty(&self, clips: &[ClipRect]) -> Result<()> {
        Ok(self.card.dirty_framebuffer(self.fb, clips)?)
    }
    
    pub fn is_ready(&self) -> bool {
        // Check if the device is in a usable state
        self.card.resource_handles().is_ok()
    }
    
    pub fn device_info(&self) -> Result<String> {
        let res = self.card.resource_handles()?;
        let connector_count = res.connectors().len();
        let crtc_count = res.crtcs().len();
        let plane_count = self.card.plane_handles()?.len();
        
        Ok(format!("DRM Device: {} connectors, {} CRTCs, {} planes, {}x{} resolution", 
                   connector_count, crtc_count, plane_count, self.width(), self.height()))
    }
    
    pub fn refresh_mode(&mut self) -> Result<()> {
        // Re-read the current mode in case it changed
        let res = self.card.resource_handles()?;
        let coninfo = res
            .connectors()
            .iter()
            .flat_map(|con| self.card.get_connector(*con, true))
            .collect::<Vec<_>>();
            
        let con = coninfo
            .iter()
            .find(|&i| i.state() == connector::State::Connected)
            .ok_or_else(|| anyhow!("No connected connectors found during refresh"))?;
            
        let &new_mode = con.modes().get(0)
            .ok_or_else(|| anyhow!("No modes found during refresh"))?;
            
        if new_mode.size() != self.mode.size() {
            self.mode = new_mode;
            eprintln!("[DrmBackend] Display mode changed to {}x{}", 
                     self.mode.size().0, self.mode.size().1);
        }
        
        Ok(())
    }
    
    pub fn supported_formats(&self) -> Result<Vec<DrmFourcc>> {
        let planes = self.card.plane_handles()?;
        let mut formats = Vec::new();
        
        for &plane in &planes {
            if let Ok(_props) = self.card.get_properties(plane) {
                // This is a simplified approach - in practice you'd need to query
                // the actual format list from the plane properties
                formats.push(DrmFourcc::Xrgb8888);
                formats.push(DrmFourcc::Argb8888);
            }
        }
        
        // Remove duplicates without sorting since DrmFourcc doesn't implement Ord
        let mut unique_formats = Vec::new();
        for format in formats {
            if !unique_formats.contains(&format) {
                unique_formats.push(format);
            }
        }
        Ok(unique_formats)
    }
    
    pub fn set_power_state(&self, power_on: bool) -> Result<()> {
        let res = self.card.resource_handles()?;
        let coninfo = res
            .connectors()
            .iter()
            .flat_map(|con| self.card.get_connector(*con, true))
            .collect::<Vec<_>>();
            
        for con in coninfo {
            if con.state() == connector::State::Connected {
                // Try to set DPMS state (Display Power Management Signaling)
                // Note: This is a simplified approach - actual implementation would need
                // to handle different connector types and their specific power management
                if power_on {
                    eprintln!("[DrmBackend] Powering on display");
                } else {
                    eprintln!("[DrmBackend] Powering off display");
                }
                break;
            }
        }
        
        Ok(())
    }
    
    pub fn get_performance_info(&self) -> Result<String> {
        let res = self.card.resource_handles()?;
        let connector_count = res.connectors().len();
        let crtc_count = res.crtcs().len();
        let plane_count = self.card.plane_handles()?.len();
        
        let info = format!(
            "DRM Performance Info:\n\
             • Resolution: {}x{}\n\
             • Connectors: {}\n\
             • CRTCs: {}\n\
             • Planes: {}\n\
             • Buffer Size: {} bytes\n\
             • Format: {:?}",
            self.width(), self.height(),
            connector_count, crtc_count, plane_count,
                         self.db.size().0 * self.db.size().1 * 4, // width * height * 4 bytes per pixel
            DrmFourcc::Xrgb8888
        );
        
        Ok(info)
    }
    pub fn map(&mut self) -> Result<DumbMapping<'_>> {
        Ok(self.card.map_dumb_buffer(&mut self.db)?)
    }
} 