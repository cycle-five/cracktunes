pub struct Wide {
    pub f1: u32,
    pub f2: u32,
    pub f3: u32,
    pub f4: u32,
    pub f5: u32,
    pub f6: u32,
    pub f7: u32,
    pub f8: u32,
    pub f9: u32,
    pub f10: u32,
}

pub fn read_a(w: &Wide) -> u32 {
    w.f1 + w.f2
}
