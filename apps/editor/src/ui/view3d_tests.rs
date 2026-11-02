use super::*;

#[test]
fn virtual_keys_map_to_platform_keys() {
    // Which position W maps to depends on the active layout.
    assert!(matches!(platform_key(Key::W), Some(PKey::Char(_))));
    assert_eq!(platform_key(Key::from_code(0x35)), Some(PKey::Char('5')));
    assert!(!is_fly_key(Key::B));
    assert_eq!(platform_key(Key::ESCAPE), Some(PKey::Escape));
    assert_eq!(platform_key(Key::F5), Some(PKey::F(5)));
    assert_eq!(platform_key(Key::LEFT), Some(PKey::Left));
    assert_eq!(platform_key(Key::from_code(0xFF)), None);
}
