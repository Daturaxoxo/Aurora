use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use log::*;
use shared::classes::games::{identity::GameId, registry};
use slint::VecModel;
use crate::bridge::Bridge;
use crate::runtime::{self, Admission};
use crate::translations::tr;
use crate::{GameChoice, MainWindow};

include!(concat!(env!("OUT_DIR"), "/game_tiles.rs"));

thread_local! {
    static TILES: RefCell<HashMap<&'static str, Option<slint::Image>>> = RefCell::new(HashMap::new());
}

pub struct GamesHandler;
impl GamesHandler {
    pub fn setup(window: &slint::Weak<MainWindow>) {
        let w = window.unwrap();
        let ww = window.clone();
        w.on_game_selected(move |id| Self::select(&ww, id.as_str()));
        Self::refresh(&w);
    }

    pub fn refresh(w: &MainWindow) {
        let snapshot = runtime::get().ok().map(runtime::GameRuntime::snapshot);
        let selected = snapshot.as_ref().map(|s| s.game.descriptor().id.clone());
        let busy = snapshot
            .as_ref()
            .is_none_or(|s| matches!(s.admission, Admission::Starting | Admission::Transitioning));

        let choices: Vec<GameChoice> = registry()
            .iter()
            .map(|game| {
                let d = game.descriptor();
                GameChoice {
                    id: d.id.as_str().into(),
                    name: d.display_name.into(),
                    image: Self::tile(d.id.as_str()).unwrap_or_default(),
                    selected: selected.as_ref() == Some(&d.id),
                    disabled: d.engine.is_none(),
                }
            })
            .collect();

        w.set_games_busy(busy);
        w.set_game_choices(Rc::new(VecModel::from(choices)).into());
    }

    fn select(window: &slint::Weak<MainWindow>, id: &str) {
        let Some(w) = window.upgrade() else { return };
        let Ok(id) = GameId::parse(id) else {
            error!("The game selector sent an invalid id '{id}'");
            return;
        };
        let Ok(runtime) = runtime::get() else {return};

        if runtime.snapshot().game.descriptor().id == id {
            w.set_show_games(false);
            return;
        }
        if runtime.session_active() {
            Bridge::show_toast(window, &tr("toast.game-switch-session"), "warning");
            return;
        }

        info!("Switching to {id} from the game selector");
        w.set_show_games(false);
        w.set_games_busy(true);
        std::thread::spawn(move || {
            if let Err(e) = runtime.switch(&id) {
                warn!("Could not switch to {id}: {e:#}");
            }
        });
    }

    fn tile(id: &str) -> Option<slint::Image> {
        let (id, bytes) = GAME_TILES.iter().find(|(name, _)| *name == id)?;
        TILES.with(|cache| {
            if let Some(cached) = cache.borrow().get(id) {
                return cached.clone();
            }
            let image = Self::decode(id, bytes);
            cache.borrow_mut().insert(id, image.clone());
            image
        })
    }

    fn decode(id: &str, bytes: &[u8]) -> Option<slint::Image> {
        let decoded = image::load_from_memory(bytes)
            .map_err(|e| warn!("Could not decode the tile for {id}: {e}"))
            .ok()?
            .into_rgba8();

        let (width, height) = decoded.dimensions();
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&decoded, width, height);
        Some(slint::Image::from_rgba8(buffer))
    }
}
