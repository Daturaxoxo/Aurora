MAIN_STYLE = """
/* The Topbar */
/* Background is handled by OverlayWidget so this stays transparent.
   The border still gives a subtle separator line. */
#TopBar {
    background-color: transparent;
    border-bottom: 1px solid rgba(255, 255, 255, 15);
}

/* Icon Buttons (Settings, Folder, Coffee, Min/Close) */
QPushButton {
    background-color: transparent;
    border: none;
    padding: 5px;
}

QPushButton:hover {
    background-color: rgba(255, 255, 255, 20);
    border-radius: 5px;
}

QToolTip {
    background-color: #141414;
    color: #D7D7D7;
    border: 1px solid #202020;
    font-family: 'Segoe UI', system-ui, sans-serif;
    font-size: 13px;
    font-weight: bold;
    padding: 6px;
    opacity: 230;
}

/* Launch Button */
#LaunchButton {
    background-color: #141414;
    color: #D7D7D7;
    border: 4px solid #202020;
    border-radius: 25px;
    font-weight: bold;
    font-size: 18px;
    padding-left: 20px;
    text-align: left;
}

#LaunchButton:hover {
    background-color: #1a1a1a;
    border-color: #2a2a2a;
}

#LaunchButton:disabled {
    background-color: #0a0a0a;
    color: #4B4B4B;
}

/* Search Button — sits left of Launch when path is missing */
#SearchButton {
    background-color: #141414;
    border: 4px solid #202020;
    border-radius: 25px;
    padding: 0px;
}

#SearchButton:hover {
    background-color: #1a1a1a;
    border-color: #2a2a2a;
}

#SearchButton:disabled {
    background-color: #0a0a0a;
}
"""

SETTING_STYLE = """
#SettingsContainer {
    background-color: #101010;
    border: 1px solid #333333;
    border-radius: 20px;
}

#SettingsHeader {
    font-size: 24px;
    font-weight: bold;
    color: #D7D7D7;
}

#SidebarButton {
    text-align: left;
    padding: 10px 20px;
    font-size: 16px;
    color: #969696;
    border-radius: 10px;
}

#SidebarButton:hover {
    background-color: rgba(255, 255, 255, 10);
}

#SidebarButton[active="true"] {
    color: #D7D7D7;
    background-color: rgba(255, 255, 255, 15);
}
"""

TOAST_STYLE = """
#ToastContainer {
    background-color: rgba(25, 20, 35, 230);
    border: 1px solid #4B4B4B;
    border-radius: 10px;
}
#ToastMessage {
    color: #D7D7D7;
    font-size: 14px;
    font-weight: 500;
}
"""

POPUP_STYLE = """
#PopupContainer {
    background-color: #101010;
    border: 1px solid #333333;
    border-radius: 20px;
}

#PopupTitle {
    font-size: 20px;
    font-weight: bold;
    color: #D7D7D7;
}

#PopupMessage {
    font-size: 14px;
    color: #969696;
}

#PopupConfirmButton {
    background-color: #D7D7D7;
    color: #101010;
    border: none;
    border-radius: 10px;
    font-size: 14px;
    font-weight: bold;
    padding: 8px 24px;
}

#PopupConfirmButton:hover {
    background-color: #ffffff;
}

#PopupCancelButton {
    background-color: transparent;
    color: #969696;
    border: 1px solid #333333;
    border-radius: 10px;
    font-size: 14px;
    padding: 8px 24px;
}

#PopupCancelButton:hover {
    background-color: rgba(255, 255, 255, 10);
    color: #D7D7D7;
}

/* Dim overlay behind the popup */
#DimOverlay {
    background-color: rgba(0, 0, 0, 140);
    border-radius: 10px;
}
"""

MOD_MANAGER_STYLE = """
/* ── Outer container — mirrors SettingsContainer ── */
#ModManagerOverlay {
    background-color: #101010;
    border: 1px solid #333333;
    border-radius: 20px;
}

/* ── Header bar ── */
#ModManagerHeader {
    background-color: rgba(255, 255, 255, 3);
    border-bottom: 1px solid rgba(255, 255, 255, 8);
    border-top-left-radius: 20px;
    border-top-right-radius: 20px;
}

#ModManagerTitle {
    color: #D7D7D7;
    font-size: 20px;
    font-weight: 500;
}

#ModCount {
    color: #484848;
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 1px;
}

/* ── Close button in header ── */
#ModManagerClose {
    background-color: transparent;
    border: none;
    border-radius: 8px;
    color: #585858;
    font-size: 18px;
    padding: 0px;
}

#ModManagerClose:hover {
    background-color: rgba(255, 255, 255, 8);
    color: #D7D7D7;
}

/* ── Search row (pill bar + icon buttons) ── */
#SearchRow {
    background-color: rgba(255, 255, 255, 4);
    border: 1px solid rgba(255, 255, 255, 7);
    border-radius: 12px;
}

#ModSearch {
    background-color: transparent;
    border: none;
    color: #D7D7D7;
    font-size: 13px;
    padding: 0px 6px;
    selection-background-color: rgba(255, 255, 255, 15);
}

#ModSearch:focus {
    background-color: transparent;
    border: none;
    outline: none;
}

#SearchIcon {
    color: #484848;
    font-size: 13px;
    background: transparent;
    border: none;
    padding: 0px;
}

/* Divider between search and action buttons */
#SearchDivider {
    background-color: rgba(255, 255, 255, 8);
    max-width: 1px;
    min-width: 1px;
}

/* Refresh + open-folder icon buttons inside the search row */
#SearchActionBtn {
    background-color: transparent;
    border: none;
    border-radius: 7px;
    padding: 4px;
}

#SearchActionBtn:hover {
    background-color: rgba(255, 255, 255, 10);
}

#SearchActionBtn:pressed {
    background-color: rgba(255, 255, 255, 5);
}

/* ── Mod image thumbnail ── */
#ModImage {
    border-radius: 6px;
    background-color: rgba(255, 255, 255, 6);
    border: 1px solid rgba(255, 255, 255, 8);
}

/* ── Mod cards — mirrors SettingRow ── */
#ModCard {
    background-color: rgba(255, 255, 255, 4);
    border: 1px solid rgba(255, 255, 255, 7);
    border-radius: 10px;
}

#ModCard:hover {
    background-color: rgba(255, 255, 255, 7);
    border: 1px solid rgba(255, 255, 255, 12);
}

#ModTitle {
    color: #E8E8E8;
    font-size: 14px;
    font-weight: 500;
}

#ModMeta {
    color: #707070;
    font-size: 12px;
}

#ModAuthorLink {
    color: #5B9BD5;
    font-size: 12px;
    text-decoration: underline;
}

#ModAuthorLink:hover {
    color: #7DB8F0;
}

#ModVersion {
    color: #484848;
    font-family: 'Consolas', monospace;
    font-size: 11px;
}

/* ── Empty-state label ── */
#EmptyLabel {
    color: #484848;
    font-size: 13px;
}

/* ── Scroll area ── */
QScrollArea {
    background: transparent;
    border: none;
}

#ScrollContent {
    background: transparent;
}

QScrollBar:vertical {
    border: none;
    background: transparent;
    width: 6px;
    margin: 4px 0;
}

QScrollBar::handle:vertical {
    background: rgba(255, 255, 255, 18);
    border-radius: 3px;
    min-height: 24px;
}

QScrollBar::handle:vertical:hover {
    background: rgba(255, 255, 255, 35);
}

QScrollBar::add-line:vertical,
QScrollBar::sub-line:vertical {
    height: 0px;
}
"""
