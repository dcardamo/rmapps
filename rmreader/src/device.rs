//! reMarkable device specs → page geometry in PDF points (72 pt/inch).

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Device {
    pub key: &'static str,
    pub name: &'static str,
    pub width_px: u32,  // short edge (portrait)
    pub height_px: u32, // long edge (portrait)
    pub ppi: u32,
}

impl Device {
    pub fn width_pt(&self) -> f32 {
        self.width_px as f32 / self.ppi as f32 * 72.0
    }
    pub fn height_pt(&self) -> f32 {
        self.height_px as f32 / self.ppi as f32 * 72.0
    }
}

pub const MOVE: Device = Device {
    key: "paper-pro-move",
    name: "reMarkable Paper Pro Move",
    width_px: 954,
    height_px: 1696,
    ppi: 264,
};

pub const PRO: Device = Device {
    key: "paper-pro",
    name: "reMarkable Paper Pro",
    width_px: 1620,
    height_px: 2160,
    ppi: 229,
};

pub fn get_device(key: &str) -> anyhow::Result<Device> {
    match key {
        "paper-pro-move" => Ok(MOVE),
        "paper-pro" => Ok(PRO),
        other => anyhow::bail!("unknown device {other:?}; choices: paper-pro-move, paper-pro"),
    }
}
