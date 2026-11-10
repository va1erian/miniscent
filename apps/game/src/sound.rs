//! Sound effects for what happens in the game.
//!
//! The session turns each [`GameEvent`] into at most one sound here. Sound
//! only listens: nothing in the simulation, a script or a replay reads it, so a
//! muted run, a build without audio and a headless run all play the same game.
//! The sounds are Freedoom's, in `assets/sounds` (see its README for the
//! license).

use mscent_audio::{Play, SoundBank};
use mscent_game::{Event as GameEvent, Game};
use mscent_math::Vec3;

/// A sound the game can make, and the file it plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    PlayerShot,
    EnemyShot,
    EnemyHurt,
    EnemyDeath,
    PlayerHurt,
    PlayerDeath,
    Win,
    Lose,
    Restart,
    MoverStart,
    MoverBlocked,
    MoverStruck,
    PropPicked,
    PropUsed,
    PropStruck,
    PropDestroyed,
    Spawn,
    /// The pilot left the ship or boarded it.
    Hatch,
}

impl Cue {
    /// The bank name of the sound: a Freedoom sound lump's name.
    pub fn file(self) -> &'static str {
        match self {
            Self::PlayerShot => "dspistol",
            Self::EnemyShot => "dsfirsht",
            Self::EnemyHurt => "dspopain",
            Self::EnemyDeath => "dspodth1",
            Self::PlayerHurt => "dsplpain",
            Self::PlayerDeath => "dspldeth",
            Self::Win => "dsgetpow",
            Self::Lose | Self::MoverBlocked => "dsnoway",
            Self::Restart => "dspstart",
            Self::MoverStart => "dsstnmov",
            Self::MoverStruck => "dsmetal",
            Self::PropPicked => "dsitemup",
            Self::PropUsed => "dsswtchn",
            Self::PropStruck => "dstink",
            Self::PropDestroyed => "dsbarexp",
            Self::Spawn => "dstelept",
            Self::Hatch => "dsstnmov",
        }
    }

    /// Every cue, for tests and for checking a bank has what the game needs.
    pub const ALL: [Cue; 18] = [
        Self::PlayerShot,
        Self::EnemyShot,
        Self::EnemyHurt,
        Self::EnemyDeath,
        Self::PlayerHurt,
        Self::PlayerDeath,
        Self::Win,
        Self::Lose,
        Self::Restart,
        Self::MoverStart,
        Self::MoverBlocked,
        Self::MoverStruck,
        Self::PropPicked,
        Self::PropUsed,
        Self::PropStruck,
        Self::PropDestroyed,
        Self::Spawn,
        Self::Hatch,
    ];
}

/// Distance at which a sound has fallen to half volume, in world units.
const HALF_VOLUME_DISTANCE: f32 = 16.0;

/// Where a sound is relative to the listener.
fn spatial(game: &Game, at: Vec3) -> (f32, f32) {
    let offset = at - game.actor_pos();
    let gain = 1.0 / (1.0 + offset.length() / HALF_VOLUME_DISTANCE);
    // Straight ahead or behind has no side; only the right-hand part pans.
    let pan = offset
        .normalized()
        .map_or(0.0, |n| n.dot(game.actor_right()));
    (gain, pan)
}

/// The sound for an event and where it came from, if it has one.
fn cue_for(game: &Game, event: &GameEvent) -> Option<(Cue, Option<Vec3>)> {
    let enemy_at = |i: usize| game.enemies.get(i).map(|e| e.pos());
    let prop_at = |i: usize| game.props.get(i).map(|p| p.pos);
    Some(match *event {
        GameEvent::PlayerFired => (Cue::PlayerShot, None),
        GameEvent::EnemyFired(i) => (Cue::EnemyShot, enemy_at(i)),
        // A killing blow reports a hit too; the death is the sound for it.
        GameEvent::EnemyHit(i) if game.enemies.get(i).is_some_and(|e| e.alive()) => {
            (Cue::EnemyHurt, enemy_at(i))
        }
        GameEvent::EnemyKilled(i) => (Cue::EnemyDeath, enemy_at(i)),
        GameEvent::PlayerHurt(_) => (Cue::PlayerHurt, None),
        GameEvent::PlayerDied => (Cue::PlayerDeath, None),
        GameEvent::GoalReached | GameEvent::ScriptWon => (Cue::Win, None),
        GameEvent::ScriptLost => (Cue::Lose, None),
        GameEvent::Restarted => (Cue::Restart, None),
        GameEvent::MoverSent(_) => (Cue::MoverStart, None),
        GameEvent::MoverBlocked(_) => (Cue::MoverBlocked, None),
        GameEvent::MoverHit(_) => (Cue::MoverStruck, None),
        GameEvent::EnemySpawned(i) => (Cue::Spawn, enemy_at(i)),
        GameEvent::PropPicked(i) => (Cue::PropPicked, prop_at(i)),
        GameEvent::PropUsed(i) => (Cue::PropUsed, prop_at(i)),
        GameEvent::PropHit(i) => (Cue::PropStruck, prop_at(i)),
        GameEvent::PropDestroyed(i) => (Cue::PropDestroyed, prop_at(i)),
        GameEvent::Disembarked | GameEvent::Boarded => (Cue::Hatch, Some(game.player.pos())),
        GameEvent::BoardRefused => (Cue::MoverBlocked, None),
        _ => return None,
    })
}

/// Plays the game's sounds into a sink: the audio device, or a test's list.
pub struct Sounds {
    bank: SoundBank,
    sink: Box<dyn FnMut(Play) + Send>,
}

impl Sounds {
    pub fn new(bank: SoundBank, sink: impl FnMut(Play) + Send + 'static) -> Self {
        Self {
            bank,
            sink: Box::new(sink),
        }
    }

    /// The cues the bank has no sound for. A level of the game stays playable
    /// without them; the host says so once at start.
    pub fn missing(&self) -> Vec<&'static str> {
        Cue::ALL
            .iter()
            .map(|c| c.file())
            .filter(|f| self.bank.get(f).is_none())
            .collect()
    }

    /// Plays whatever `event` sounds like. `game` is as it is after the event.
    pub fn on_event(&mut self, game: &Game, event: &GameEvent) {
        let Some((cue, at)) = cue_for(game, event) else {
            return;
        };
        let Some(sound) = self.bank.get(cue.file()) else {
            return;
        };
        let (gain, pan) = at.map_or((1.0, 0.0), |at| spatial(game, at));
        (self.sink)(Play {
            sound: sound.clone(),
            gain,
            pan,
        });
    }
}

/// Opens the default audio device with the game's sounds, or says why there
/// will be none. Missing files are reported but do not stop the others.
#[cfg(feature = "audio")]
pub fn open(assets_root: Option<&std::path::Path>) -> Result<Sounds, String> {
    let (bank, skipped) = crate::assets::load_sounds(assets_root)?;
    for why in &skipped {
        eprintln!("sound skipped: {why}");
    }
    let output = mscent_audio::device::Output::open()?;
    let sounds = Sounds::new(bank, move |req| output.play(req));
    for file in sounds.missing() {
        eprintln!("sound missing: {file}.wav");
    }
    Ok(sounds)
}

/// Without the `audio` feature (the LazyOS build) the game is silent.
#[cfg(not(feature = "audio"))]
pub fn open(_: Option<&std::path::Path>) -> Result<Sounds, String> {
    Err("built without the `audio` feature".into())
}
