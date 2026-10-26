# Android client notes

## User-facing strings (i18n)

All UI text lives in `app/src/main/res/values/strings.xml` and uses the web client's key mirrored by `toAndroidName` (C15): `.` becomes `_` and camelCase becomes snake_case (`connection.retryNow` is `connection_retry_now`); web plurals `x.one`/`x.other` are one `<plurals name="x">`; placeholders are `%1$s`/`%1$d`. ViewModels expose `@StringRes Int` (never English text or server `message` strings) and Compose reads plurals with `pluralStringResource`. A `values-<lang>/strings.xml` folder must be empty or translate every string and plural (C17), and a language goes into `locales_config.xml` only once complete; `bun run i18n:check` in `client/` enforces this (see `docs/developer-guide/development/i18n.md`).
