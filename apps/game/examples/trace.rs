use miniscent_game::{GameSession, SessionConfig, assets};
use mscent_platform::{Action, Script};
use mscent_render::Quality;
use std::time::Duration;
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let script = Script::parse(&std::fs::read_to_string(&a[1]).unwrap()).unwrap();
    let cfg = SessionConfig {
        quality: Quality::Low,
        ..SessionConfig::default()
    };
    let m = assets::load_materials(None).unwrap();
    let mut s = GameSession::new(&mscent_world::showcase_level(), m, &cfg).unwrap();
    let dt = Duration::from_micros(16_667);
    let acts = script.actions().to_vec();
    let mut i = 0;
    for f in 0..1700u32 {
        let t = f as f64 * 0.016667;
        let mut ev = vec![];
        while i < acts.len() && acts[i].0 <= t {
            if let Action::Input(e) = acts[i].1 {
                ev.push(e);
            }
            i += 1;
        }
        s.step(dt, &ev);
        if f % 30 == 0 {
            let p = s.game().player.pos();
            let fw = s.game().player.forward();
            println!(
                "{:5.1}s pos ({:5.1},{:5.1},{:4.1}) fwd ({:6.3},{:6.3},{:6.3}) hp {} goal {:.1} {:?} en {:?}",
                t,
                p.x,
                p.y,
                p.z,
                fw.x,
                fw.y,
                fw.z,
                s.game().player.health,
                s.game().goal_distance(),
                s.phase(),
                s.game()
                    .enemies
                    .iter()
                    .map(|e| (e.state, e.pos().x as i32, e.pos().y as i32, e.hp))
                    .collect::<Vec<_>>()
            );
        }
    }
}
