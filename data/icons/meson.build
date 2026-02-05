install_data(
  '@0@.svg'.format(application_id),
  install_dir: iconsdir / 'hicolor' / 'scalable' / 'apps'
)

install_data(
  '@0@-symbolic.svg'.format(base_id),
  install_dir: iconsdir / 'hicolor' / 'symbolic' / 'apps',
  rename: '@0@-symbolic.svg'.format(application_id)
)

# Symbolic icons used inside the app UI (gtk::Image::from_icon_name).
tool_symbolic_icons = [
  'claude-code-symbolic.svg',
  'codex-symbolic.svg',
  'mistral-vibe-symbolic.svg',
  'opencode-symbolic.svg',
]

foreach icon : tool_symbolic_icons
  install_data(
    icon,
    install_dir: iconsdir / 'hicolor' / 'symbolic' / 'apps',
  )
endforeach
