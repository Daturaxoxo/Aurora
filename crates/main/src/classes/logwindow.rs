use std::{cell::RefCell, fmt::Write as _, rc::Rc, time::Duration};

use log::{debug, error, info, Level};
use shared::{
    config::{self, key},
    logger::{self, LogEntry, LOG_BUFFER_CAPACITY},
};
use slint::{Color, ComponentHandle, Model, ModelRc, SharedString, Timer, TimerMode, VecModel};

use crate::{LogLine, LogWindow, MainWindow};

const POLL_INTERVAL: Duration = Duration::from_millis(200);

const CHAR_WIDTH: f32 = 6.6;

/// Timestamp column + level column + the layout's padding and gaps.
const FIXED_COLUMNS_WIDTH: f32 = 128.0 + 46.0 + 20.0 + 30.0;

const MODULE_COLUMN_MIN: f32 = 200.0;

struct View {
    model: Rc<VecModel<LogLine>>,
    cursor: u64,
    /// 0 = All, otherwise the single level being shown.
    filter: i32,
    module_chars: usize,
    message_chars: usize,
    applied: (usize, usize),
}

struct State {
    window: LogWindow,
    view: Rc<RefCell<View>>,
    timer: Timer,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

pub fn set_visible(visible: bool, main: &slint::Weak<MainWindow>) {
    if visible {
        open(main);
    } else {
        hide();
    }
}

pub fn apply_language(lang_code: &str) {
    STATE.with_borrow(|state| {
        if let Some(state) = state {
            crate::translations::apply_language_to_log_window(&state.window, lang_code);
        }
    });
}

pub fn hide() {
    let window = STATE.with_borrow(|state| {
        let state = state.as_ref()?;
        state.timer.stop();
        Some(state.window.clone_strong())
    });

    let Some(window) = window else {
        return;
    };

    if let Err(e) = window.hide() {
        error!("[LogWindow] could not hide the log window: {e}");
        return;
    }

    info!("[LogWindow] log window hidden");
}

fn stop_polling() {
    STATE.with_borrow(|state| {
        if let Some(state) = state {
            state.timer.stop();
        }
    });
}

fn clear_developer_mode(main: &slint::Weak<MainWindow>) {
    config::set(key::DEV_MODE, false);
    if let Some(w) = main.upgrade() {
        w.set_developer_mode(false);
    } else {
        error!("[LogWindow] window handle dead, could not clear the Developer Mode toggle");
    }
}

fn open(main: &slint::Weak<MainWindow>) {
    if STATE.with_borrow(Option::is_none) {
        match build(main) {
            Ok(state) => STATE.with_borrow_mut(|slot| *slot = Some(state)),
            Err(e) => {
                error!("[LogWindow] could not create the log window: {e}");
                return;
            }
        }
    }

    let handles = STATE.with_borrow(|state| {
        state
            .as_ref()
            .map(|state| (state.window.clone_strong(), state.view.clone()))
    });

    let Some((window, view)) = handles else {
        return;
    };

    rebuild(&view, &window);

    if let Err(e) = window.show() {
        error!("[LogWindow] could not show the log window: {e}");
        return;
    }

    window.set_follow(true);
    window.invoke_scroll_to_bottom();

    STATE.with_borrow(|state| {
        if let Some(state) = state {
            start_polling(state);
        }
    });

    info!("[LogWindow] log window opened");
}

fn start_polling(state: &State) {
    let view = state.view.clone();
    let window = state.window.as_weak();

    state
        .timer
        .start(TimerMode::Repeated, POLL_INTERVAL, move || {
            let Some(window) = window.upgrade() else {
                return;
            };
            append_new(&view, &window);
        });
}

fn build(main: &slint::Weak<MainWindow>) -> Result<State, slint::PlatformError> {
    let window = LogWindow::new()?;
    window.set_ui_font_family("Segoe UI".into());
    crate::translations::apply_saved_language_to_log_window(&window);
    let view = Rc::new(RefCell::new(View {
        model: Rc::new(VecModel::default()),
        cursor: 0,
        filter: 0,
        module_chars: 0,
        message_chars: 0,
        applied: (usize::MAX, usize::MAX),
    }));

    window.set_lines(ModelRc::from(view.borrow().model.clone()));

    let ww = window.as_weak();
    window.on_window_dragged(move |delta_x, delta_y| {
        let Some(w) = ww.upgrade() else { return };
        let position = w.window().position();
        #[allow(clippy::cast_precision_loss)]
        w.window()
            .set_position(slint::WindowPosition::Logical(slint::LogicalPosition::new(
                position.x as f32 + delta_x,
                position.y as f32 + delta_y,
            )));
    });

    let ww = window.as_weak();
    window.on_minimize_clicked(move || {
        if let Some(w) = ww.upgrade() {
            w.window().set_minimized(true);
        }
    });

    let ww = window.as_weak();
    window.on_maximize_clicked(move || {
        if let Some(w) = ww.upgrade() {
            w.window().set_maximized(!w.window().is_maximized());
        }
    });

    let mm = main.clone();
    window.on_close_window(move || {
        info!("[LogWindow] closed from the title bar, turning Developer Mode off");
        hide();
        clear_developer_mode(&mm);
    });

    let mm = main.clone();
    window.window().on_close_requested(move || {
        info!("[LogWindow] closed from outside the app, turning Developer Mode off");
        stop_polling();
        clear_developer_mode(&mm);
        slint::CloseRequestResponse::HideWindow
    });

    let vv = view.clone();
    let ww = window.as_weak();
    window.on_level_filter_changed(move |index| {
        info!("[LogWindow] level filter changed → {index}");
        let Some(w) = ww.upgrade() else { return };
        vv.borrow_mut().filter = index;
        rebuild(&vv, &w);
        if w.get_follow() {
            w.invoke_scroll_to_bottom();
        }
    });

    let vv = view.clone();
    window.on_copy_logs(move || {
        copy_logs(vv.borrow().filter);
    });

    Ok(State {
        window,
        view,
        timer: Timer::default(),
    })
}

fn rebuild(view: &Rc<RefCell<View>>, window: &LogWindow) {
    let mut v = view.borrow_mut();
    let filter = v.filter;

    let mut rows = Vec::new();
    let mut module_chars = 0;
    let mut message_chars = 0;

    v.cursor = logger::for_each_log_since(0, |entry| {
        if !passes(filter, entry.level) {
            return;
        }
        let row = to_row(entry);
        module_chars = module_chars.max(entry.module.chars().count());
        message_chars = message_chars.max(row.message.chars().count());
        rows.push(row);
    });

    let count = rows.len();
    v.module_chars = module_chars;
    v.message_chars = message_chars;
    v.model.set_vec(rows);
    apply_widths(&mut v, window);
    drop(v);

    window.set_line_count(i32::try_from(count).unwrap_or(i32::MAX));
    debug!("[LogWindow] rebuilt view with {count} row(s)");
}

fn append_new(view: &Rc<RefCell<View>>, window: &LogWindow) {
    let mut v = view.borrow_mut();
    let filter = v.filter;

    let mut rows = Vec::new();
    let mut module_chars = v.module_chars;
    let mut message_chars = v.message_chars;

    let cursor = logger::for_each_log_since(v.cursor, |entry| {
        if !passes(filter, entry.level) {
            return;
        }
        let row = to_row(entry);
        module_chars = module_chars.max(entry.module.chars().count());
        message_chars = message_chars.max(row.message.chars().count());
        rows.push(row);
    });

    v.cursor = cursor;
    if rows.is_empty() {
        return;
    }

    v.module_chars = module_chars;
    v.message_chars = message_chars;
    v.model.extend(rows);

    while v.model.row_count() > LOG_BUFFER_CAPACITY {
        v.model.remove(0);
    }

    let count = v.model.row_count();
    apply_widths(&mut v, window);
    drop(v);

    window.set_line_count(i32::try_from(count).unwrap_or(i32::MAX));
}

#[allow(clippy::cast_precision_loss)]
fn apply_widths(view: &mut View, window: &LogWindow) {
    if view.applied == (view.module_chars, view.message_chars) {
        return;
    }
    view.applied = (view.module_chars, view.message_chars);

    let module_width = (view.module_chars as f32 * CHAR_WIDTH).max(MODULE_COLUMN_MIN);
    window.set_module_column_width(module_width);
    window.set_content_width(
        (view.message_chars as f32).mul_add(CHAR_WIDTH, FIXED_COLUMNS_WIDTH + module_width),
    );
}

fn copy_logs(filter: i32) {
    let mut text = String::new();
    logger::for_each_log_since(0, |entry| {
        if !passes(filter, entry.level) {
            return;
        }
        let _ = writeln!(
            text,
            "[{} {} {}] {}",
            entry.timestamp, entry.level, entry.module, entry.message
        );
    });

    match arboard::Clipboard::new().and_then(|mut c| c.set_text(text)) {
        Ok(()) => info!("[LogWindow] logs copied to clipboard"),
        Err(e) => error!("[LogWindow] could not copy logs to clipboard: {e}"),
    }
}

fn to_row(entry: &LogEntry) -> LogLine {
    LogLine {
        timestamp: SharedString::from(entry.timestamp.as_str()),
        level: SharedString::from(entry.level.as_str()),
        module: SharedString::from(entry.module.as_str()),
        message: single_line(&entry.message),
        color: level_color(entry.level),
    }
}

fn single_line(message: &str) -> SharedString {
    if !message.contains(['\n', '\r', '\t']) {
        return SharedString::from(message);
    }

    SharedString::from(
        message
            .chars()
            .map(|c| {
                if matches!(c, '\n' | '\r' | '\t') {
                    ' '
                } else {
                    c
                }
            })
            .collect::<String>(),
    )
}

const fn passes(filter: i32, level: Level) -> bool {
    filter == 0 || filter == level_index(level)
}

const fn level_index(level: Level) -> i32 {
    match level {
        Level::Error => 1,
        Level::Warn => 2,
        Level::Info => 3,
        Level::Debug => 4,
        Level::Trace => 5,
    }
}

const fn level_color(level: Level) -> Color {
    match level {
        Level::Error => Color::from_rgb_u8(255, 0, 0),
        Level::Warn => Color::from_rgb_u8(255, 255, 0),
        Level::Info => Color::from_rgb_u8(79, 184, 150),
        Level::Debug => Color::from_rgb_u8(0, 255, 255),
        Level::Trace => Color::from_rgb_u8(0, 0, 255),
    }
}
