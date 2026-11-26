use std::collections::HashSet;

use super::*;

fn letters(grid: &Grid) -> String {
    grid.iter().map(|row| row.iter().map(|ink| ink.letter()).collect::<String>() + "\n").collect()
}

fn fingerprint(grid: &Grid) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for ink in grid.iter().flatten() {
        hash ^= u64::from(ink.letter() as u32);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

const APPROVED_SITTING: &[(Variant, State, [u64; FRAMES])] = &[
    (
        Variant::Zen,
        State::Idle,
        [
            0xde3291dd6724e0f9,
            0xde3291dd6724e0f9,
            0x77717e66a2aa50f9,
            0x77717e66a2aa50f9,
            0x77717e66a2aa50f9,
            0x77717e66a2aa50f9,
            0xde3291dd6724e0f9,
            0xde3291dd6724e0f9,
        ],
    ),
    (
        Variant::Zen,
        State::Work,
        [
            0xb5665f951942af41,
            0xb5665f951942af41,
            0x5536dfc722d63341,
            0x5536dfc722d63341,
            0x5536dfc722d63341,
            0x5536dfc722d63341,
            0xde3291dd6724e0f9,
            0xb5665f951942af41,
        ],
    ),
    (
        Variant::GoGarden,
        State::Idle,
        [
            0x0e7b2257c83e44b1,
            0x1553549bf27a7a13,
            0x67578fafde509ca4,
            0xc1fc9985624778e6,
            0xbaec4edca88dd104,
            0x05e749ab79bb813e,
            0x4b4fba5c48e65b76,
            0xec5f33a8544f2f12,
        ],
    ),
    (
        Variant::GoGarden,
        State::Work,
        [
            0xd2275a51d5780b19,
            0xad1f7e89af7d6c4b,
            0xbe2b88928ec27a6c,
            0x6070d41fb3de41ae,
            0xbdfc8c9cd1922cdc,
            0x2948d267ad070716,
            0x4b4fba5c48e65b76,
            0x1d05d6b981f2b20a,
        ],
    ),
    (
        Variant::RustRonin,
        State::Idle,
        [
            0xc316957d6509ddc0,
            0xc77dd635af52dfac,
            0xc14b9ac051aa49d5,
            0x8114d64c5d674fe9,
            0x0f9e6784337b9f8d,
            0x01861e3371085501,
            0x051bf56a0ec24ec0,
            0x128a41f3da21ddfe,
        ],
    ),
    (
        Variant::RustRonin,
        State::Work,
        [
            0x6e64ca1bef6a2c08,
            0xfb336e0518292e54,
            0xb4abd5b27d4d487d,
            0x818f3b87991e39f1,
            0x49d9dff23f934ef5,
            0xa9bdf571e5a7b5c9,
            0x051bf56a0ec24ec0,
            0x004490e203b573e6,
        ],
    ),
    (
        Variant::PyPetal,
        State::Idle,
        [
            0x5a942f8b625e07d5,
            0x86940b74b72e3ae5,
            0x5b103097e824cec3,
            0x7624f4eb344ba68e,
            0x7fe1b04eda3e06af,
            0x02517ecda68ecb53,
            0xc6b1e09e04201e9b,
            0xdd5ff9454f6e88e3,
        ],
    ),
    (
        Variant::PyPetal,
        State::Work,
        [
            0xbb6fe9cc5f32e46d,
            0xa683226c2171b8fd,
            0x1e4f0331a307bb6b,
            0x24a85b24327ddea6,
            0x55ca00ea958da7e7,
            0xc11df12ca7462c2b,
            0xc6b1e09e04201e9b,
            0x066353c5978032db,
        ],
    ),
    (
        Variant::JvmJizo,
        State::Idle,
        [
            0xcc14f811b0699065,
            0x8975520d76f53181,
            0x7d79d499b9df4ec1,
            0xa31537d413e45e69,
            0x0cd7a8c70b791011,
            0xa28a846186b19535,
            0x2b4d5ef1f2688060,
            0x32f6803243c46038,
        ],
    ),
    (
        Variant::JvmJizo,
        State::Work,
        [
            0x1d72d5923106dffd,
            0x6616fb492d75acc1,
            0x058a9e1c918b95e9,
            0x51e1d5f649f743e1,
            0xfa4f39b07dcf88c9,
            0x1efd46626537680d,
            0x2b4d5ef1f2688060,
            0xed691001d2b01ee0,
        ],
    ),
    (
        Variant::WebWave,
        State::Idle,
        [
            0x66e2609dab78bbb6,
            0x9011ccb578706251,
            0x818635cac70688e6,
            0x26778f452f4865a0,
            0x0c694a9845c6eb40,
            0xecfb4d8722615f06,
            0x9011ccb578706251,
            0x17d4cade91b7edd6,
        ],
    ),
    (
        Variant::WebWave,
        State::Work,
        [
            0x80b5f5130559e55e,
            0x8fc2e495abd66009,
            0xcdd18636f082d7ce,
            0xf20ac30c0a45a808,
            0xd187c50add148b68,
            0xfa48c3b195e871ee,
            0x9011ccb578706251,
            0x9ef883952dd1207e,
        ],
    ),
    (
        Variant::CppCrane,
        State::Idle,
        [
            0xed12f91ef740a3ee,
            0xed12f91ef740a3ee,
            0x6ad1deabeca3c59c,
            0x6ad1deabeca3c59c,
            0x8573ef91c1fa7db0,
            0x8573ef91c1fa7db0,
            0xc1f74d820f5347fa,
            0xc1f74d820f5347fa,
        ],
    ),
    (
        Variant::CppCrane,
        State::Work,
        [
            0x27235d5345884886,
            0x27235d5345884886,
            0x90b1f3548a3f09f4,
            0x90b1f3548a3f09f4,
            0xf92dbfe9ddec5c98,
            0xf92dbfe9ddec5c98,
            0xc1f74d820f5347fa,
            0x79e2b3072b4bc122,
        ],
    ),
    (
        Variant::SharpShuriken,
        State::Idle,
        [
            0x0fb35a925e8b707f,
            0x3d2663a2cabea738,
            0xba29fc5aea656666,
            0x76914c86a5a3022d,
            0xba29fc5aea656666,
            0x76914c86a5a3022d,
            0x0fb35a925e8b707f,
            0x3d2663a2cabea738,
        ],
    ),
    (
        Variant::SharpShuriken,
        State::Work,
        [
            0xef161bc9013baa97,
            0x2cfe691408203c30,
            0x8913b01125faa93e,
            0x8e3c378896947165,
            0x8913b01125faa93e,
            0x8e3c378896947165,
            0x0fb35a925e8b707f,
            0x2cfe691408203c30,
        ],
    ),
    (
        Variant::PhpPagoda,
        State::Idle,
        [
            0x81db36048cd02754,
            0x3535a460a2ff0e59,
            0x87e87b020f7807b1,
            0x87e87b020f7807b1,
            0xd49ec37e5dbe4fa8,
            0x87e87b020f7807b1,
            0x3535a460a2ff0e59,
            0x3535a460a2ff0e59,
        ],
    ),
    (
        Variant::PhpPagoda,
        State::Work,
        [
            0xe56434dcb44730ec,
            0x52a9859403f2e231,
            0xce4caef90c39d9d9,
            0xce4caef90c39d9d9,
            0xa0d62ea79e177270,
            0xce4caef90c39d9d9,
            0x3535a460a2ff0e59,
            0x52a9859403f2e231,
        ],
    ),
    (
        Variant::RubyRyokan,
        State::Idle,
        [
            0x6b724970c67f0312,
            0x47aeaa85441b0c44,
            0x5ba091c97e1da6ac,
            0xb78d83efcb660d4a,
            0xd9ca1fd9eee51f8a,
            0x081422ec7eb62150,
            0x76dadd5ed6b5f1d0,
            0x921cbb21d506e952,
        ],
    ),
    (
        Variant::RubyRyokan,
        State::Work,
        [
            0x2b1ac0656648b4ca,
            0x98eed21c3473c03c,
            0x1a5e1d222a10a004,
            0x9e7ecadaf419aeb2,
            0xaf66e9b9456d3a12,
            0xae7c3eb62b151048,
            0x76dadd5ed6b5f1d0,
            0xe08cdb9c5f0e2a0a,
        ],
    ),
    (
        Variant::SqlScroll,
        State::Idle,
        [
            0xc64746fa2509d9f2,
            0x48ca36fd6e2d035e,
            0xf0c380438ecd8523,
            0x1c11eb79eccb516a,
            0x1c11eb79eccb516a,
            0xf0c380438ecd8523,
            0x48ca36fd6e2d035e,
            0xc64746fa2509d9f2,
        ],
    ),
    (
        Variant::SqlScroll,
        State::Work,
        [
            0x3de00241f4b1e69a,
            0x7060af065c687ee6,
            0x4ae9be78eb3915eb,
            0x4858587146b0b692,
            0x4858587146b0b692,
            0x4ae9be78eb3915eb,
            0x48ca36fd6e2d035e,
            0x3de00241f4b1e69a,
        ],
    ),
    (
        Variant::DevopsDaruma,
        State::Idle,
        [
            0xa3658de218fcab7f,
            0x7d061bb9f81f258b,
            0x4382e9ed18d0c42f,
            0x47fa999003b9eac7,
            0x1ddfd319f6492d1f,
            0x81774244a269299e,
            0xbf3b6d8ef5c16b17,
            0xdbd9f1404129a83b,
        ],
    ),
    (
        Variant::DevopsDaruma,
        State::Work,
        [
            0x05a0b4a02fe5ebf7,
            0x5d5eeb7806c60ee3,
            0x53608f2b44e9fd27,
            0x99836f0f2502067f,
            0x6f2527d7d8045fa7,
            0x30f9f2aa99386c26,
            0xbf3b6d8ef5c16b17,
            0xa42d566adcf70b73,
        ],
    ),
    (
        Variant::TestTotami,
        State::Idle,
        [
            0x2881217e0b83d6e0,
            0xbe7ecb9c4cbef8c0,
            0x21487abab2f6f0c8,
            0x42b4aba3e4d47042,
            0xa7fc82178c1c2779,
            0x82a632edc1211f19,
            0xe69a367fbd884d2c,
            0x1d7f01cbb50d4b85,
        ],
    ),
    (
        Variant::TestTotami,
        State::Work,
        [
            0x1d89c0117fba6e38,
            0x8d336e152f366c58,
            0xbdb5e4caa85ee650,
            0x72821a29baa5d0ca,
            0x23ee00fcf3415c81,
            0xc5c54e2c91e0a581,
            0xe69a367fbd884d2c,
            0xa5a7b8f0fffca06d,
        ],
    ),
    (
        Variant::Toolbox,
        State::Idle,
        [
            0x0fd278d0953b897c,
            0x0fd278d0953b897c,
            0x498629ad91c7b334,
            0x0522793e7d39bb74,
            0x3310a68439e7d464,
            0xb7c4c6c3c6adc13c,
            0x0fd278d0953b897c,
            0x0fd278d0953b897c,
        ],
    ),
    (
        Variant::Toolbox,
        State::Work,
        [
            0xadb31f0b8a820974,
            0xadb31f0b8a820974,
            0xf4f2d70032d024bc,
            0x9b51d85f8632067c,
            0xdb73eb7c7f093fac,
            0x9b60cd31d855ef84,
            0x0fd278d0953b897c,
            0xadb31f0b8a820974,
        ],
    ),
];
const APPROVED_OTHER: &[(State, [u64; FRAMES])] = &[
    (
        State::Success,
        [
            0x1534388174d5050d,
            0x466b83693c6cdf5d,
            0x7f1147bcd47343ad,
            0x21bac395bb57914d,
            0x276e8fef0a1af11d,
            0xe8a863ef36678815,
            0x92855f73d02d690d,
            0x8cde6f7d876a5f6d,
        ],
    ),
    (
        State::Error,
        [
            0x504d9a9e4b2351d9,
            0x6599c7b63808c899,
            0x504d9a9e4b2351d9,
            0x9732a40929d463f0,
            0xba20baebfb404e07,
            0x9005470f3fb4de47,
            0x173f478435e019ae,
            0x26297d9301e1add2,
        ],
    ),
    (
        State::Sleep,
        [
            0x6901d019c78c5e18,
            0x9baa2cf8bb7e5859,
            0xf2eeba97f8646bb1,
            0x6c4865b4577995cb,
            0x2316a7d2fe7b3d67,
            0xbe093a9b1896f817,
            0x1eb4916aa75d1e48,
            0x978c33057ef708b8,
        ],
    ),
];

#[test]
fn every_frame_is_the_approved_one_pixel_for_pixel() {
    for (variant, state, approved) in APPROVED_SITTING {
        for (t, approved) in approved.iter().enumerate() {
            let grid = frame(*variant, *state, t);
            assert_eq!(fingerprint(&grid), *approved, "{variant:?} {state:?} frame {t}:\n{}", letters(&grid));
        }
    }
    for (state, approved) in APPROVED_OTHER {
        for variant in Variant::ALL {
            for (t, approved) in approved.iter().enumerate() {
                let grid = frame(variant, *state, t);
                assert_eq!(fingerprint(&grid), *approved, "{variant:?} {state:?} frame {t}:\n{}", letters(&grid));
            }
        }
    }
}

#[test]
fn key_pixels_of_the_family_kozo_at_rest() {
    let grid = frame(Variant::Zen, State::Idle, STILL);
    let at = |x: usize, y: usize| grid[y][x];
    assert_eq!(at(17, 5), Ink::Outline, "the top of the head");
    assert_eq!(at(16, 7), Ink::Shine, "the shine on the head");
    assert_eq!(at(15, 14), Ink::Eye, "the left eye, closed");
    assert_eq!(at(20, 14), Ink::Skin, "between the eyes");
    assert_eq!(at(14, 16), Ink::Cheek, "the left cheek");
    assert_eq!(at(16, 21), Ink::White, "the collar");
    assert_eq!(at(14, 26), Ink::Obi, "the obi");
    assert_eq!(at(0, 0), Ink::Clear, "the corner is clear");
}

#[test]
fn kozo_breathes_he_rises_a_pixel_and_comes_back() {
    let rest = frame(Variant::Zen, State::Idle, 0);
    let breath = frame(Variant::Zen, State::Idle, 3);
    assert_eq!(rest[5][17], Ink::Outline);
    assert_eq!(breath[6][17], Ink::Outline, "a pixel lower: the head sinks as he breathes out");
    assert_eq!(frame(Variant::Zen, State::Idle, 7), rest, "the loop ends where it starts");
}

#[test]
fn the_outline_closes_round_everything_solid() {
    let closed = |grid: &Grid, what: &str| {
        for y in 0..SIZE {
            for x in 0..SIZE {
                if grid[y][x].floats() {
                    continue;
                }
                let neighbours = [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)];
                for (nx, ny) in neighbours {
                    if nx < SIZE && ny < SIZE {
                        assert_ne!(
                            grid[ny][nx],
                            Ink::Clear,
                            "{what}: ({x}, {y}) is open at ({nx}, {ny}):\n{}",
                            letters(grid)
                        );
                    }
                }
            }
        }
    };
    for variant in Variant::ALL {
        if variant == Variant::WebWave {
            continue;
        }
        for t in 0..FRAMES {
            closed(&frame(variant, State::Idle, t), &format!("{variant:?} idle {t}"));
            closed(&frame(variant, State::Work, t), &format!("{variant:?} work {t}"));
        }
    }
    for t in 0..FRAMES {
        closed(&frame(Variant::Zen, State::Success, t), &format!("success {t}"));
        closed(&frame(Variant::Zen, State::Sleep, t), &format!("sleep {t}"));
    }
}

#[test]
fn every_product_has_its_own_kozo_and_its_prop_moves() {
    let family = frame(Variant::Zen, State::Idle, STILL);
    let mut stills = HashSet::new();
    for variant in Variant::ALL {
        let frames: HashSet<Grid> = (0..FRAMES).map(|t| frame(variant, State::Idle, t)).collect();
        assert!(frames.len() >= 2, "{variant:?} moves");
        assert!(stills.insert(frame(variant, State::Idle, STILL)), "{variant:?} has a still frame of its own");
        if variant != Variant::Zen {
            assert!(frames.len() > 2, "{variant:?}'s prop moves, not only his breath");
            assert_ne!(frame(variant, State::Idle, STILL), family, "{variant:?} has its prop");
        }
    }
    for state in State::ALL {
        let frames: HashSet<Grid> = (0..FRAMES).map(|t| frame(Variant::Zen, state, t)).collect();
        assert!(frames.len() >= 2, "{} moves", state.name());
    }
}

#[test]
fn a_product_id_gives_its_variant_and_others_the_family() {
    for variant in Variant::ALL {
        assert_eq!(Variant::of(variant.id()), variant);
    }
    assert_eq!(Variant::of("toolbox"), Variant::Toolbox);
    assert_eq!(Variant::of("test"), Variant::Zen);
    assert_eq!(Variant::of(""), Variant::Zen);
    assert_eq!(Variant::Toolbox.robe(), Variant::Zen.robe(), "the toolbox wears the family's indigo");
    assert_eq!(Ink::Robe.rgb(Variant::GoGarden), Some([0x5E, 0xE7, 0xD0]));
    assert_eq!(Ink::Clear.rgb(Variant::GoGarden), None);
}

#[test]
fn letters_and_inks_go_both_ways() {
    for letter in "osShwbcez!*gGdDlLpPxXyYtTuUknvrR.".chars() {
        assert_eq!(Ink::from_letter(letter).letter(), letter);
    }
}

#[test]
fn the_animation_loops_every_eight_frames_of_160_ms() {
    assert_eq!(frame_at(0), 0);
    assert_eq!(frame_at(159), 0);
    assert_eq!(frame_at(160), 1);
    assert_eq!(frame_at(160 * 8), 0);
    assert_eq!(frame(Variant::GoGarden, State::Idle, 9), frame(Variant::GoGarden, State::Idle, 1));
}
