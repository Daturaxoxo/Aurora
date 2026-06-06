# Aurora Logger
import logging, os, platform, threading, time, shutil, ctypes, subprocess
from datetime import datetime, timezone
from PyQt6.QtCore import QObject, pyqtSignal
from src.utils import get_app_dir
from src import config_manager as cfg
from pathlib import Path
from src.backend.helpers.paths import CLIENT_PAK_DIR
from pathlib import Path
from src.backend.helpers.paths import detect_version, get_version_paths, CLIENT_PAK_DIR

class ExtensiveLoggingFilter(logging.Filter):
    def filter(self, record):
        if getattr(record, 'el', False):
            return cfg.get(cfg.Key.EXTENSIVE_LOGGING)
        return True
    
class ErrorTriggeredFileHandler(logging.Handler):
    def __init__(self, log_dir):
        super().__init__(level=logging.DEBUG)
        self.log_dir = log_dir
        self.buffer  = []
        self.file_created = False
        self.log_path     = None
        self.formatter = logging.Formatter(
            '[AU] [%(asctime)s] [%(levelname)s] %(message)s',
            datefmt='%H:%M:%S'
        )

    def emit(self, record):
        self.buffer.append(record)
        if record.levelno >= logging.ERROR and not self.file_created: self.create_file()
        if self.file_created: self.write(record)

    def create_file(self):
        try:
            os.makedirs(self.log_dir, exist_ok=True)
            timestamp     = datetime.now().strftime("aurora_%Y-%m-%d_%H-%M-%S.log")
            self.log_path = os.path.join(self.log_dir, timestamp)
            self.file_created = True
            with open(self.log_path, 'w', encoding='utf-8') as f:
                f.write(f"--- Aurora Error Log — {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} ---\n")
                f.write("(Full session history leading up to the error)\n\n")
                for record in self.buffer: f.write(self.formatter.format(record) + '\n')
        except Exception: pass

    def write(self, record):
        try: 
            with open(self.log_path, 'a', encoding='utf-8') as f: f.write(self.formatter.format(record) + '\n')
        except Exception: pass

class _ConsoleSignaller(QObject): append_html = pyqtSignal(str)

class DevConsoleHandler(logging.Handler):
    def __init__(self):
        super().__init__(level=logging.DEBUG)
        self._widget    = None
        self._signaller = None
        self.session_buffer: list[logging.LogRecord] = []
        self.formatter = logging.Formatter(
            '[%(asctime)s] [%(levelname)s] %(message)s',
            datefmt='%H:%M:%S'
        )
        self._colours = {
            logging.DEBUG:    "#969696",
            logging.INFO:     "#D7D7D7",
            logging.WARNING:  "#f6c177",
            logging.ERROR:    "#eb6f92",
            logging.CRITICAL: "#eb6f92",
        }

    def attach(self, widget, history: list = None):
        self._widget = widget
        if self._signaller is None: self._signaller = _ConsoleSignaller()
        self._signaller.append_html.connect(widget.append)
        el_enabled = cfg.get(cfg.Key.EXTENSIVE_LOGGING)
        for record in self.session_buffer:
            is_el = getattr(record, 'el', False)
            if is_el and not el_enabled: continue
            widget.append(self.format_html(record))

    def detach(self):
        if self._widget is not None:
            try: self._signaller.append_html.disconnect(self._widget.append)
            except Exception: pass
        self._widget = None

    def emit(self, record):
        is_el = getattr(record, 'el', False)
        self.session_buffer.append(record)
        if is_el and not cfg.get(cfg.Key.EXTENSIVE_LOGGING): return
        if self._widget is None or self._signaller is None: return
        try: self._signaller.append_html.emit(self.format_html(record))
        except Exception: pass
        
    def format_html(self, record) -> str:
        colour = self._colours.get(record.levelno, "#D7D7D7")
        msg    = self.formatter.format(record).replace("<", "&lt;").replace(">", "&gt;")
        return f'<span style="color:{colour}">{msg}</span>'

def collect_system_info() -> str:
    try:
        from src.path_finder import get_local_version
        aurora_version = get_local_version()
    except Exception: aurora_version = "<unknown>"
    lines = [
        "=== System Information ===",
        f"Aurora version  : {aurora_version}",
        f"OS              : {platform.system()} {platform.release()}",
        f"OS Version      : {platform.version()}",
        f"Architecture    : {platform.machine()} / {platform.processor()}",
        f"Python          : {platform.python_version()}",
        f"Timezone (local): {datetime.now().astimezone().tzname()} "
        f"(UTC{datetime.now(timezone.utc).astimezone().strftime('%z')})",
        f"Hostname        : {platform.node()}",
    ]
    return '\n'.join(lines)

def collect_game_info() -> str:
    try:
        from src.backend.helpers.paths import detect_version
        game_path = cfg.get(cfg.Key.GAME_PATH)
        if not game_path: return "=== Game Information ===\nGame path not set."
        version = detect_version(Path(game_path))
        
        mod_folder = Path(game_path) / CLIENT_PAK_DIR
        if mod_folder.is_dir():
            mod_files  = [f for f in mod_folder.rglob("*") if f.is_file()]
            mod_count  = len(mod_files)
            mod_bytes  = sum(f.stat().st_size for f in mod_files)
            mod_size   = f"{mod_bytes / (1024 ** 2):.1f} MB" if mod_bytes >= 1024 ** 2 else f"{mod_bytes / 1024:.1f} KB"
            mod_detail = f"{mod_count} file(s), {mod_size}"
        else: mod_detail = "<unknown folder>"

        try:
            write_ok = os.access(game_path, os.W_OK)
            write_str = "YES" if write_ok else "NO  < may cause access denied errors"
        except Exception: write_str = "<could not check>"

        try:
            usage     = shutil.disk_usage(game_path)
            free_gb   = usage.free  / (1024 ** 3)
            total_gb  = usage.total / (1024 ** 3)
            disk_str  = f"{free_gb:.1f} GB free of {total_gb:.1f} GB"
        except Exception: disk_str = "<could not check>"

        lines = [
            "=== Game Information ===",
            f"Game path       : {game_path}",
            f"Detected version: {version}",
            f"Write access    : {write_str}",
            f"Disk space      : {disk_str}",
            f"Mods loaded     : {mod_detail}",
        ]
        return '\n'.join(lines)
    except Exception as exc: return f"=== Game Information ===\n(Failed to collect: {exc})"

def collect_environment() -> str:
    lines = ["=== Environment ==="]

    try:
        is_admin = bool(ctypes.windll.shell32.IsUserAnAdmin())
        lines.append(f"Admin privileges: {'YES' if is_admin else 'NO <- UAC might block DLL writes'}")
    except Exception:
        try:
            is_admin = os.getuid() == 0
            lines.append(f"Admin privileges: {'YES' if is_admin else 'NO'}")
        except Exception: lines.append("Admin privileges: <could not check>")

    try:
        result = subprocess.run(
            ["powershell", "-NoProfile", "-Command",
             "(Get-MpComputerStatus).RealTimeProtectionEnabled"],
            capture_output=True, text=True, timeout=5
        )
        val = result.stdout.strip().lower()
        if val == "true": lines.append("Windows Defender : ON  <- If the user is reporting missing files, its most likely that defender has removed them.")
        elif val == "false": lines.append("Windows Defender : OFF")
        else: lines.append(f"Windows Defender : <could not check>")
    except Exception: lines.append("Windows Defender : <could not check>")
    return '\n'.join(lines)


def collect_aurora_config() -> str:
    lines = ["=== Aurora Configuration ==="]
    try:
        keys_to_log = [
            (cfg.Key.CENSORSHIP_REMOVE, "Censorship remover"),
            (cfg.Key.NO_DRIVE_LINE,     "No drive line"),
            (cfg.Key.ENGINE_METHOD,     "Engine method"),
            (cfg.Key.EXTENSIVE_LOGGING, "Extensive logging"),
            (cfg.Key.DISCORD_RPC,       "Discord RPC"),
        ]
        for key, label in keys_to_log:
            try:
                val = cfg.get(key)
                lines.append(f"  {label:<22}: {val}")
            except Exception: lines.append(f"  {label:<22}: <unavailable>")
    except Exception as exc: lines.append(f"  (Failed to collect: {exc})")
    return '\n'.join(lines)

def collect_file_status() -> str:
    lines = ["=== File Status (snapshot at export time) ==="]

    def entry(label: str, path: Path | str):
        path = Path(path)
        if path.is_file():
            size  = path.stat().st_size
            mtime = datetime.fromtimestamp(path.stat().st_mtime).strftime('%Y-%m-%d %H:%M:%S')
            lines.append(f"  {label}: EXISTS  ({size} bytes, modified {mtime})  [{path}]")
        elif path.is_dir():
            try: count = sum(len(f) for _, _, f in os.walk(path))
            except Exception: count = '?'
            lines.append(f"  {label}: DIR  ({count} files)  [{path}]")
        else: lines.append(f"  {label}: MISSING  [{path}]")

    game_path_str = cfg.get(cfg.Key.GAME_PATH) or ""

    if game_path_str:
        game_path = Path(game_path_str)
        try:
            version       = detect_version(game_path)
            engine_method = cfg.get(cfg.Key.ENGINE_METHOD) or "0"
            vpaths        = get_version_paths(game_path, version, engine_method)

            for slot in vpaths.dll_slots:
                for label, path in slot.all_targets: entry(f"DLL [{slot.name} / {label}]", path)
            entry("ASI plugin", vpaths.asi_plugin)
        except Exception as exc: lines.append(f"  Game files: <could not resolve — {exc}>")

        entry("Mod folder", game_path / CLIENT_PAK_DIR)
    else:
        lines.append("  Game files: <game path not set> [ACTION:SKIP]")
        lines.append("  Mod folder: <game path not set> [ACTION:SKIP]")

    try:
        config_path = Path(get_app_dir()) / "config.json"
        entry("config.json", config_path)
    except Exception as exc: lines.append(f"  config.json: <unavailable — {exc}>")

    return '\n'.join(lines)


def format_session_log(records: list[logging.LogRecord]) -> str:
    formatter = logging.Formatter(
        '[%(asctime)s] [%(levelname)s] %(message)s',
        datefmt='%H:%M:%S'
    )
    lines = ["=== Session Log ==="]
    for r in records: lines.append(formatter.format(r))
    return '\n'.join(lines)


def format_file_status_sections() -> str:
    sections = []
    inject = getattr(file_monitor, 'inject_snapshot', None) if file_monitor else None
    sections.append("===== File Status [INJECT] =====")
    sections.append(inject if inject else "  <no injection recorded this session>")
    sections.append("")
    sections.append("===== File Status [PERIODIC] =====")
    periodic = getattr(file_monitor, 'periodic_entries', []) if file_monitor else []
    if periodic: sections.append('\n\n'.join(periodic))
    else: sections.append("  <no periodic entries recorded this session>")
    sections.append("")
    sections.append("===== File Status [SNAPSHOT] =====")
    watch_targets = getattr(file_monitor, '_last_watch_targets', None) if file_monitor else None
    if watch_targets:
        lines = []
        for label, path in watch_targets:
            path = path if hasattr(path, 'stat') else Path(path)
            if path.is_file():
                size  = path.stat().st_size
                mtime = datetime.fromtimestamp(path.stat().st_mtime).strftime('%Y-%m-%d %H:%M:%S')
                lines.append(f"  {label}: EXISTS  ({size} bytes, modified {mtime})  [{path}]")
            elif path.is_dir(): lines.append(f"  {label}: DIR  [{path}]")
            else: lines.append(f"  {label}: MISSING  [{path}]")
        full = collect_file_status()
        for line in full.splitlines():
            if any(k in line for k in ("Mod folder", "config.json")): lines.append(line)
        sections.append('\n'.join(lines))
    else:
        snapshot = collect_file_status()
        sections.append(snapshot.replace("=== File Status (snapshot at export time) ===\n", ""))

    return '\n'.join(sections)


def export_telemetry(out_path: str | None = None) -> str:
    app_dir   = get_app_dir()
    log_dir   = os.path.join(app_dir, "Logs")
    os.makedirs(log_dir, exist_ok=True)

    if out_path is None:
        timestamp = datetime.now().strftime("aurora_telemetry_%Y-%m-%d_%H-%M-%S.aulog")
        out_path  = os.path.join(log_dir, timestamp)

    sections = [
        f"Aurora Telemetry Export",
        f"Generated : {datetime.now().strftime('%Y-%m-%d %H:%M:%S')} "
          f"(local) / {datetime.now(timezone.utc).strftime('%Y-%m-%d %H:%M:%S')} (UTC)",
        "",
        collect_system_info(),
        "",
        collect_environment(),
        "",
        collect_aurora_config(),
        "",
        collect_game_info(),
        "",
        format_file_status_sections(),
        "",
        format_session_log(dev_console_handler.session_buffer if dev_console_handler else []),
    ]

    content = '\n'.join(sections)
    with open(out_path, 'w', encoding='utf-8') as f: f.write(content)
    return out_path

class FileStatusMonitor:
    def __init__(self, interval: int = 5):
        self.interval   = interval
        self.thread: threading.Thread | None = None
        self.stop_event = threading.Event()
        self.last_config_mtime: float | None = None
        self.watch_targets: list[tuple[str, object]] | None = None
        self.last_watch_targets: list[tuple[str, object]] | None = None
        self.watch_interval: int = 10
        self.inject_snapshot: str | None = None
        self.periodic_entries: list[str] = []

    def start(self):
        if self.thread and self.thread.is_alive(): return
        self.stop_event.clear()
        self.thread = threading.Thread(target=self.run, daemon=True, name="AuroraFileMonitor")
        self.thread.start()

    def stop(self): self.stop_event.set()

    def start_injection_watch(self, vpaths, asi_path):
        targets = []
        for slot in vpaths.dll_slots:
            for label, path in slot.all_targets: targets.append((f"DLL [{slot.name}/{label}]", path))
        targets.append(("ASI plugin", asi_path))
        self.watch_targets = targets
        self.last_watch_targets = targets
        self.inject_snapshot = self.format_watch_snapshot(
            f"Inject — {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"
        )
        self.periodic_entries.clear()

        logging.getLogger("Aurora").info(
            f"File monitor: watching {len(targets)} injection target(s) every {self.watch_interval}s.",
            extra={'el': True}
        )

    def stop_injection_watch(self): self.watch_targets = None

    def run(self):
        elapsed = 0
        while not self.stop_event.wait(1):
            elapsed += 1
            if self.watch_targets is not None:
                if elapsed >= self.watch_interval:
                    elapsed = 0
                    self._tick_watch()
            elif elapsed >= self.interval:
                elapsed = 0
                self._tick()

    def format_watch_snapshot(self, label: str) -> str:
        if not self.watch_targets: return f"[{label}] <no targets>"
        lines = [f"[{label}]"]
        for entry_label, path in self.watch_targets:
            if path.is_file():
                size  = path.stat().st_size
                mtime = datetime.fromtimestamp(path.stat().st_mtime).strftime('%Y-%m-%d %H:%M:%S')
                lines.append(f"  {entry_label}: EXISTS  ({size} bytes, modified {mtime})  [{path}]")
            elif path.is_dir(): lines.append(f"  {entry_label}: DIR  [{path}]")
            else: lines.append(f"  {entry_label}: MISSING  [{path}]")
        return '\n'.join(lines)

    def _tick_watch(self):
        logger = logging.getLogger("Aurora")
        timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        entry = self.format_watch_snapshot(timestamp)
        self.periodic_entries.append(entry)
        for entry_label, path in self.watch_targets:
            if not path.exists(): logger.warning(f"[File Monitor] {entry_label} is no longer present: {path}")

    def _tick(self):
        global file_handler
        if file_handler is None or not file_handler.file_created: return

        try:
            snapshot  = collect_file_status()
            timestamp = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
            block     = f"\n[{timestamp}] --- Periodic File Status ---\n{snapshot}\n"

            try:
                app_dir     = get_app_dir()
                config_path = os.path.join(app_dir, "config.json")
                mtime       = os.path.getmtime(config_path) if os.path.isfile(config_path) else None
                if mtime is not None and mtime != self.last_config_mtime:
                    if self.last_config_mtime is not None: block += f"  [!] config.json changed since last snapshot\n"
                    self.last_config_mtime = mtime
            except Exception: pass

            with open(file_handler.log_path, 'a', encoding='utf-8') as f: f.write(block)
        except Exception: pass

def setup_logger():
    app_dir = get_app_dir()
    log_dir = os.path.join(app_dir, "Logs")
    root_logger = logging.getLogger()
    if root_logger.handlers: return logging.getLogger("Aurora")

    console_handler = logging.StreamHandler()
    console_handler.setFormatter(logging.Formatter(
        '[AU] [%(asctime)s] [%(levelname)s] %(message)s',
        datefmt='%H:%M:%S'
    ))

    global file_handler
    file_handler = ErrorTriggeredFileHandler(log_dir)
    global dev_console_handler
    dev_console_handler = DevConsoleHandler()
    el_filter = ExtensiveLoggingFilter()
    root_logger.setLevel(logging.DEBUG)
    root_logger.addHandler(console_handler)
    root_logger.addHandler(file_handler)
    root_logger.addHandler(dev_console_handler)

    logger = logging.getLogger("Aurora")
    logger.addFilter(el_filter)
    logger.info("————— Aurora Launcher —————")
    logger.info(f"App directory: {app_dir}", extra={'el': True})

    global file_monitor
    file_monitor = FileStatusMonitor(interval=5)
    file_monitor.start()
    return logger

dev_console_handler: DevConsoleHandler          = None
file_handler:        ErrorTriggeredFileHandler  = None
file_monitor:       FileStatusMonitor         = None
logger = setup_logger()

def InitFatalError(): pass