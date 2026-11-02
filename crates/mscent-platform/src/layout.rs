//! Keyboard layout awareness for hosts that report layout-dependent keys.
//!
//! The game binds movement to key *positions* (the ones labelled WASD on a
//! QWERTY keyboard), so on an AZERTY keyboard the same keys read ZQSD. The
//! desktop backend already receives positions from winit. A host that only
//! sees the letter printed on the key (the editor's Win32 virtual-key codes)
//! uses [`physical_letter`] to turn that letter back into its position.

/// Hardware scancodes of the letter keys, as the QWERTY letter at that
/// position (set 1 scancodes, which Windows uses for `MapVirtualKeyW`).
const POSITIONS: [(u32, char); 26] = [
    (0x10, 'Q'),
    (0x11, 'W'),
    (0x12, 'E'),
    (0x13, 'R'),
    (0x14, 'T'),
    (0x15, 'Y'),
    (0x16, 'U'),
    (0x17, 'I'),
    (0x18, 'O'),
    (0x19, 'P'),
    (0x1E, 'A'),
    (0x1F, 'S'),
    (0x20, 'D'),
    (0x21, 'F'),
    (0x22, 'G'),
    (0x23, 'H'),
    (0x24, 'J'),
    (0x25, 'K'),
    (0x26, 'L'),
    (0x2C, 'Z'),
    (0x2D, 'X'),
    (0x2E, 'C'),
    (0x2F, 'V'),
    (0x30, 'B'),
    (0x31, 'N'),
    (0x32, 'M'),
];

/// The QWERTY letter at the position of the key that produces `letter` under
/// the active layout. `vk_of_scancode` answers what virtual key (an
/// upper-case ASCII letter for letters) the layout puts at a scancode, or 0.
/// Falls back to `letter` itself when the layout has no such key.
pub fn physical_letter_with(letter: char, vk_of_scancode: impl Fn(u32) -> u32) -> char {
    POSITIONS
        .iter()
        .find(|&&(sc, _)| vk_of_scancode(sc) == u32::from(letter))
        .map_or(letter, |&(_, position)| position)
}

/// [`physical_letter_with`] against the keyboard layout currently active in
/// the OS. Identity where the OS cannot be asked.
pub fn physical_letter(letter: char) -> char {
    #[cfg(windows)]
    {
        physical_letter_with(letter, os::vk_of_scancode)
    }
    #[cfg(not(windows))]
    {
        letter
    }
}

#[cfg(windows)]
#[allow(unsafe_code)] // the single FFI call below
mod os {
    #[link(name = "user32")]
    unsafe extern "system" {
        fn MapVirtualKeyW(code: u32, map_type: u32) -> u32;
    }

    /// `MAPVK_VSC_TO_VK`: scancode in, layout-specific virtual key out.
    const MAPVK_VSC_TO_VK: u32 = 1;

    pub fn vk_of_scancode(scancode: u32) -> u32 {
        // SAFETY: MapVirtualKeyW takes two plain integers, touches no memory
        // of ours and is documented to return 0 for an unmapped code.
        unsafe { MapVirtualKeyW(scancode, MAPVK_VSC_TO_VK) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An AZERTY layout swaps A/Q and Z/W and moves M; the rest match QWERTY.
    fn azerty(scancode: u32) -> u32 {
        let qwerty = POSITIONS.iter().find(|p| p.0 == scancode).map(|p| p.1);
        u32::from(match qwerty {
            Some('Q') => 'A',
            Some('A') => 'Q',
            Some('W') => 'Z',
            Some('Z') => 'W',
            Some(c) => c,
            None => return 0,
        })
    }

    #[test]
    fn azerty_zqsd_are_the_wasd_positions() {
        let pos = |c| physical_letter_with(c, azerty);
        assert_eq!(
            (pos('Z'), pos('Q'), pos('S'), pos('D')),
            ('W', 'A', 'S', 'D')
        );
        assert_eq!((pos('A'), pos('E')), ('Q', 'E'));
    }

    #[test]
    fn qwerty_is_the_identity_and_unknown_keys_pass_through() {
        let qwerty = |sc| {
            POSITIONS
                .iter()
                .find(|p| p.0 == sc)
                .map_or(0, |p| u32::from(p.1))
        };
        assert_eq!(physical_letter_with('W', qwerty), 'W');
        assert_eq!(physical_letter_with('W', |_| 0), 'W');
    }
}
