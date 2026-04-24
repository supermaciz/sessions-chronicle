# Sessions Chronicle - Landing site

Static Astro site for https://sessions-chronicle.maciz.dev, including the self-hosted Flatpak remote under `/flatpak/`.

## Prerequisites
- Node 22+
- npm 10+

## Install
`npm install`

## Dev
`npm run dev` - http://localhost:4321

## Build
`npm run build` -> `dist/`

## Preview production build
`npm run preview`

## Deploy
Push to `main`; GitHub Actions publishes via Pages.

## Font licenses
- Adwaita Sans - SIL Open Font License 1.1 (https://gitlab.gnome.org/GNOME/adwaita-fonts)
- JetBrains Mono - SIL Open Font License 1.1 (https://www.jetbrains.com/lp/mono)

## Last local verification (2026-04-21)
- Lighthouse mobile: pending manual run
- Lighthouse desktop: pending manual run
- html-validate: 0 errors
- Responsive 360/768/1024/1440: pending manual run
- Font loading on Slow 3G: pending manual run
