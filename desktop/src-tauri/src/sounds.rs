// The pad's on-device instrument menu is 10 categories x 8 slots (catalog.h).
// It announces a pick as Bank Select (CC0 = category) + Program Change (slot).
// Until the app pushes its own slot assignments to the pad, each slot maps to
// the closest General MIDI program in the SoundFont (bank 0).
pub const DEVICE_SLOTS: [[u8; 8]; 10] = [
    // Keys: GrndPno Upright SoftPno EP1 DX-EP Suitcase Organ Church
    [0, 1, 2, 4, 5, 4, 16, 19],
    // Pads: Warm Pad1 Bright Distant Ghost HarmSweep BellPad Ooh
    [89, 88, 100, 99, 94, 95, 93, 53],
    // Leads: Lead1 Lead2 CrispPWM DigiIt Butter Fairy FMRock Dome
    [80, 81, 84, 83, 82, 86, 87, 85],
    // Bass: Bass1 E-Bass Rubber FMBass1 DeepEnd DistBass Digibass PianoBass
    [38, 33, 39, 37, 32, 34, 36, 35],
    // Plucks: Bell1 FMPluck E-Guitar Clean Diamonds DelayDancer FantasyBell 80sGliss
    [14, 45, 27, 25, 10, 105, 98, 46],
    // Poly: Jupiter8 Juno60 Triton FastPoly FMPoly FatCurtis AhhPoly Gentle
    [90, 50, 51, 62, 63, 48, 52, 91],
    // Arps (no GM arps): mallets and plucked colours instead
    [81, 87, 8, 11, 12, 13, 108, 107],
    // Space: DeepNote SpaceAdv SpaceCadet Aliens Stardust GlassVox Pretty5 Vinyl
    [97, 96, 102, 103, 98, 91, 101, 122],
    // SynBass
    [38, 39, 87, 38, 39, 36, 37, 34],
    // Signature: strings, brass, solo instruments
    [48, 61, 56, 73, 65, 40, 71, 10],
];

pub fn device_slot(cat: u8, slot: u8) -> Option<u8> {
    DEVICE_SLOTS.get(cat as usize)?.get(slot as usize).copied()
}
