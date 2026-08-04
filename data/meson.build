subdir('icons')
subdir('resources')
# Desktop file
desktop_conf = configuration_data()
desktop_conf.set('icon', application_id)
desktop_file = i18n.merge_file(
  type: 'desktop',
  input: configure_file(
    input: '@0@.desktop.in.in'.format(base_id),
    output: '@BASENAME@',
    configuration: desktop_conf
  ),
  output: '@0@.desktop'.format(application_id),
  po_dir: podir,
  install: true,
  install_dir: datadir / 'applications'
)
# Validate Desktop file
if desktop_file_validate.found()
  test(
    'validate-desktop',
    desktop_file_validate,
    args: [
      desktop_file.full_path()
    ],
    depends: desktop_file,
  )
endif

service_conf = configuration_data()
service_conf.set('APP_ID', application_id)
service_conf.set('BINDIR', bindir)
service_file = configure_file(
  input: '@0@.service.in'.format(base_id),
  output: '@0@.service'.format(application_id),
  configuration: service_conf,
  install: true,
  install_dir: datadir / 'dbus-1' / 'services',
)

search_provider_bus_name = application_id + '.SearchProvider'
search_provider_object_path = '/' + application_id.replace('.', '/') + '/SearchProvider'

search_provider_conf = configuration_data()
search_provider_conf.set('APP_ID', application_id)
search_provider_conf.set('BUS_NAME', search_provider_bus_name)
search_provider_conf.set('OBJECT_PATH', search_provider_object_path)
search_provider_file = configure_file(
  input: '@0@-search-provider.ini.in'.format(base_id),
  output: '@0@-search-provider.ini'.format(application_id),
  configuration: search_provider_conf,
  install: true,
  install_dir: datadir / 'gnome-shell' / 'search-providers',
)

search_provider_service_conf = configuration_data()
search_provider_service_conf.set('BUS_NAME', search_provider_bus_name)
search_provider_service_conf.set('BINDIR', bindir)
search_provider_service_file = configure_file(
  input: '@0@.SearchProvider.service.in'.format(base_id),
  output: '@0@.service'.format(search_provider_bus_name),
  configuration: search_provider_service_conf,
  install: true,
  install_dir: datadir / 'dbus-1' / 'services',
)

test(
  'validate-activation-metadata',
  python,
  args: [
    meson.project_source_root() / 'build-aux' / 'validate-activation-metadata.py',
    desktop_file.full_path(),
    service_file.full_path(),
    application_id,
    bindir,
    search_provider_file.full_path(),
    search_provider_service_file.full_path(),
  ],
  depends: desktop_file,
)

# Appdata
appdata_conf = configuration_data()
appdata_conf.set('app-id', application_id)
appdata_conf.set('gettext-package', gettext_package)
appdata_file = i18n.merge_file(
  input: configure_file(
    input: '@0@.metainfo.xml.in.in'.format(base_id),
    output: '@BASENAME@',
    configuration: appdata_conf
  ),
  output: '@0@.metainfo.xml'.format(application_id),
  po_dir: podir,
  install: true,
  install_dir: datadir / 'metainfo'
)
# Validate Appdata
if appstreamcli.found()
  test(
    'validate-appdata', appstreamcli,
    args: [
      'validate', '--no-net', '--explain', appdata_file.full_path()
    ],
    depends: appdata_file,
  )
endif

# GSchema
gschema_conf = configuration_data()
gschema_conf.set('app-id', application_id)
gschema_conf.set('gettext-package', gettext_package)
configure_file(
  input: '@0@.gschema.xml.in'.format(base_id),
  output: '@0@.gschema.xml'.format(application_id),
  configuration: gschema_conf,
  install: true,
  install_dir: datadir / 'glib-2.0' / 'schemas'
)

# Validata GSchema
test(
  'validate-gschema', glib_compile_schemas,
  args: [
    '--strict', '--dry-run', meson.current_build_dir()
  ],
)
