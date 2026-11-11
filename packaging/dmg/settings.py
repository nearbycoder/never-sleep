"""Finder metadata written directly by dmgbuild; no Finder automation."""
from pathlib import Path

app = Path(defines["app"]).resolve()
artwork = Path(defines["background"]).resolve()
format = "UDZO"
filesystem = "HFS+"
compression_level = 9
files = [str(app)]
symlinks = {"Applications": "/Applications"}
icon = str(app / "Contents/Resources/NeverSleep.icns")
background = str(artwork)
window_rect = ((200, 180), (660, 420))
default_view = "icon-view"
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
sidebar_width = 0
include_icon_view_settings = True
include_list_view_settings = False
arrange_by = None
grid_spacing = 80
scroll_position = (0, 0)
label_pos = "bottom"
text_size = 13
icon_size = 104
show_icon_preview = False
show_item_info = False
icon_locations = {
    "Never Sleep.app": (190, 220),
    "Applications": (470, 220),
}
