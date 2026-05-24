import os
import sys
import shutil
import subprocess
import zipfile
from src.utils import resource_path
from pathlib import Path
from PyQt6.QtWidgets import (
    QWidget, QVBoxLayout, QHBoxLayout,
    QPushButton, QLabel, QFrame, QLineEdit,
    QScrollArea, QFileDialog,
)
from PyQt6.QtCore import Qt, QSize, pyqtSignal
from PyQt6.QtGui import QIcon
from src.styles import MOD_MANAGER_STYLE
from src.translator import t
from src.engine import get_app_dir
from src.ui.elements import ModCard
from src import config_manager as cfg

class InstallZone(QFrame):
    files_installed = pyqtSignal(list)

    STYLE = """
        QFrame#InstallZone {
            border: 2px dashed #3d444d;
            border-radius: 8px;
            background: transparent;
        }
        QFrame#InstallZone:hover {
            border-color: #4493f8;
            background: rgba(68, 147, 248, 0.06);
        }
        QLabel#InstallIcon {
            color: #848d97;
            font-size: 32px;
        }
        QLabel#InstallTitle {
            color: #e6edf3;
            font-size: 14px;
            font-weight: 600;
        }
        QPushButton#InstallChooseLink {
            color: #4493f8;
            background: transparent;
            border: none;
            font-size: 13px;
            text-decoration: underline;
            padding: 0;
        }
        QPushButton#InstallChooseLink:hover {
            color: #79b8ff;
        }
    """

    def __init__(self, mods_dir: Path, parent=None):
        super().__init__(parent)
        self.mods_dir = mods_dir
        self.setObjectName("InstallZone")
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setFixedHeight(110)
        self.setStyleSheet(self.STYLE)
        self.setCursor(Qt.CursorShape.PointingHandCursor)

        layout = QVBoxLayout(self)
        layout.setAlignment(Qt.AlignmentFlag.AlignCenter)
        layout.setSpacing(4)
        layout.setContentsMargins(16, 12, 16, 12)

        icon_lbl = QLabel("🗎")
        icon_lbl.setObjectName("InstallIcon")
        icon_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)

        title_lbl = QLabel(t("install_zone_title") or "Install mods ")
        title_lbl.setObjectName("InstallTitle")
        title_lbl.setAlignment(Qt.AlignmentFlag.AlignCenter)
        title_lbl.setWordWrap(True)

        sub_row = QHBoxLayout()
        sub_row.setAlignment(Qt.AlignmentFlag.AlignCenter)
        sub_row.setSpacing(4)

        choose_btn = QPushButton(t("install_zone_choose") or "choose your files")
        choose_btn.setObjectName("InstallChooseLink")
        choose_btn.setCursor(Qt.CursorShape.PointingHandCursor)
        choose_btn.clicked.connect(self._open_file_dialog)

        sub_row.addWidget(choose_btn)

        layout.addWidget(icon_lbl)
        layout.addWidget(title_lbl)
        layout.addLayout(sub_row)

    def mousePressEvent(self, event):
        if event.button() == Qt.MouseButton.LeftButton:
            self._open_file_dialog()
        super().mousePressEvent(event)

    # File / Folder dialog
    def _open_file_dialog(self):
        # Create the dialog instance
        dialog = QFileDialog(self, "Select mod files or folders") # I CBA having to translate this to every language
        dialog.setFileMode(QFileDialog.FileMode.ExistingFiles)
        dialog.setOption(QFileDialog.Option.ShowDirsOnly, False)
        dialog.setNameFilters([
            "Mod files (*.zip *.7z *.rar)",
            "All files (*)"
        ])
        
        if dialog.exec():
            selected_paths = [Path(p) for p in dialog.selectedFiles()]
            if selected_paths:
                self._install_paths(selected_paths)

    # Install Logic
    def _unique_dest(self, dest: Path) -> Path:
        counter = 2
        original = dest
        while dest.exists():
            dest = original.parent / f"{original.stem} ({counter}){original.suffix}"
            counter += 1
        return dest

    def _install_paths(self, paths: list[Path]):
        self.mods_dir.mkdir(parents=True, exist_ok=True)
        installed = []

        for src in paths:
            if src.is_dir():
                dest = self._unique_dest(self.mods_dir / src.name)
                shutil.copytree(src, dest)
                installed.append(dest)
            elif src.is_file():
                if src.suffix.lower() == ".zip":
                    zip_dest = self._unique_dest(self.mods_dir / src.stem)
                    zip_dest.mkdir(parents=True, exist_ok=True)
                    with zipfile.ZipFile(src, "r") as zf:
                        zf.extractall(zip_dest)
                    installed.append(zip_dest)
                else:
                    dest = self._unique_dest(self.mods_dir / src.name)
                    shutil.copy2(src, dest)
                    installed.append(dest)

        if installed:
            self.files_installed.emit(installed)

class ModManagerOverlay(QFrame):
    def __init__(self, parent, mod_manager):
        super().__init__(parent)
        self.setObjectName("ModManagerOverlay")
        self.manager = mod_manager

        self.setGeometry(240, 80, 800, 560)
        self.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)
        self.setStyleSheet(MOD_MANAGER_STYLE)

        root = QVBoxLayout(self)
        root.setContentsMargins(0, 0, 0, 0)
        root.setSpacing(0)

        # Header
        header = QFrame()
        header.setObjectName("ModManagerHeader")
        header.setFixedHeight(64)
        header.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        header_layout = QHBoxLayout(header)
        header_layout.setContentsMargins(28, 0, 20, 0)
        header_layout.setSpacing(12)

        title_col = QVBoxLayout()
        title_col.setSpacing(2)
        lbl_title = QLabel(t("mod_manager"))
        lbl_title.setObjectName("ModManagerTitle")
        self._lbl_mod_count = QLabel("")
        self._lbl_mod_count.setObjectName("ModCount")
        title_col.addStretch()
        title_col.addWidget(lbl_title)
        title_col.addWidget(self._lbl_mod_count)
        title_col.addStretch()

        btn_close = QPushButton("✕")
        btn_close.setObjectName("ModManagerClose")
        btn_close.setFixedSize(32, 32)
        btn_close.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_close.clicked.connect(self.hide)

        header_layout.addLayout(title_col)
        header_layout.addStretch()
        header_layout.addWidget(btn_close)

        root.addWidget(header)

        # Body
        body = QWidget()
        body_layout = QVBoxLayout(body)
        body_layout.setContentsMargins(28, 20, 28, 24)
        body_layout.setSpacing(16)

        # Search Row
        search_row = QFrame()
        search_row.setObjectName("SearchRow")
        search_row.setFixedHeight(42)
        search_row.setAttribute(Qt.WidgetAttribute.WA_StyledBackground)

        sr_layout = QHBoxLayout(search_row)
        sr_layout.setContentsMargins(14, 0, 8, 0)
        sr_layout.setSpacing(6)

        icon_lbl = QLabel()
        icon_lbl.setObjectName("SearchIcon")
        icon_lbl.setFixedSize(18, 18)
        icon_lbl.setPixmap(QIcon(resource_path("Bin/Assets/search.png")).pixmap(16, 16))

        self.search_bar = QLineEdit()
        self.search_bar.setObjectName("ModSearch")
        self.search_bar.setPlaceholderText(t("search_mods"))
        self.search_bar.textChanged.connect(self.refresh_list)

        divider = QFrame()
        divider.setObjectName("SearchDivider")
        divider.setFrameShape(QFrame.Shape.VLine)
        divider.setFixedHeight(22)

        btn_refresh = QPushButton()
        btn_refresh.setObjectName("SearchActionBtn")
        btn_refresh.setFixedSize(30, 30)
        btn_refresh.setToolTip("Refresh mod list")
        btn_refresh.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_refresh.setIcon(QIcon(resource_path("Bin/Assets/refresh.png")))
        btn_refresh.setIconSize(QSize(16, 16))
        btn_refresh.clicked.connect(self.refresh_list)

        btn_folder = QPushButton()
        btn_folder.setObjectName("SearchActionBtn")
        btn_folder.setFixedSize(30, 30)
        btn_folder.setToolTip("Open Mods folder")
        btn_folder.setCursor(Qt.CursorShape.PointingHandCursor)
        btn_folder.setIcon(QIcon(resource_path("Bin/Assets/folder.png")))
        btn_folder.setIconSize(QSize(16, 16))
        btn_folder.clicked.connect(self._open_mods_folder)

        sr_layout.addWidget(icon_lbl)
        sr_layout.addWidget(self.search_bar, 1)
        sr_layout.addWidget(divider)
        sr_layout.addWidget(btn_refresh)
        sr_layout.addWidget(btn_folder)

        # Scroll area
        self.scroll = QScrollArea()
        self.scroll.setWidgetResizable(True)
        self.list_container = QWidget()
        self.list_container.setObjectName("ScrollContent")
        self.list_layout = QVBoxLayout(self.list_container)
        self.list_layout.setSpacing(8)
        self.list_layout.setContentsMargins(0, 0, 4, 0)
        self.list_layout.setAlignment(Qt.AlignmentFlag.AlignTop)
        self.scroll.setWidget(self.list_container)

        mods_path = Path(get_app_dir()) / "Mods"
        self.install_zone = InstallZone(mods_path, self)
        self.install_zone.files_installed.connect(self._on_files_installed)

        self.list_layout.addWidget(self.install_zone)

        body_layout.addWidget(search_row)
        body_layout.addWidget(self.scroll, 1)

        root.addWidget(body, 1)
        self.refresh_list()

    def _on_files_installed(self, paths: list):
        self.refresh_list()

    def _open_mods_folder(self):
        if cfg.get(cfg.Key.USE_HARD_LINKS):
            mods_path = Path(get_app_dir()) / "Mods"
        else:
            mods_path = Path(cfg.get(cfg.Key.GAME_PATH)) / "Client/WindowsNoEditor/HT/Content/Paks/AuroraMods"
        print(mods_path)
        if not mods_path.exists():
            mods_path.mkdir(parents=True, exist_ok=True)
        if sys.platform == "win32":
            os.startfile(str(mods_path))
        elif sys.platform == "darwin":
            subprocess.Popen(["open", str(mods_path)])
        else:
            subprocess.Popen(["xdg-open", str(mods_path)])

    def _update_mod_count(self):
        mods = self.manager.scan_mods()
        total = len(mods)
        enabled = sum(1 for m in mods if m.is_enabled)
        TMP_desc_a = t("mod_manager_desc_a") or "OF"
        TMP_desc_b = t("mod_manager_desc_b") or "ENABLED"
        self._lbl_mod_count.setText(f"{enabled} {TMP_desc_a} {total} {TMP_desc_b}")

    def refresh_list(self):
        for i in reversed(range(self.list_layout.count())):
            child = self.list_layout.itemAt(i).widget()
            if child and child != self.install_zone:
                child.deleteLater()

        search_text = self.search_bar.text().lower()
        mods = self.manager.scan_mods()
        visible = [
            m for m in mods
            if search_text in m.display_name.lower() or search_text in m.author.lower()
        ]

        self._update_mod_count()
        if not visible:
            if self.list_layout.count() == 1: # Only InstallZone exists
                empty = QLabel("No mods found" if search_text else "No mods installed")
                empty.setObjectName("EmptyLabel")
                empty.setAlignment(Qt.AlignmentFlag.AlignCenter)
                self.list_layout.addStretch()
                self.list_layout.addWidget(empty)
                self.list_layout.addStretch()
            return

        for mod in visible:
            card = ModCard(mod, self.manager, self)
            self.list_layout.addWidget(card)