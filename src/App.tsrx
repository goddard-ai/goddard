import { routes } from './routes'

// The root _layout is the app shell — the route manifest catalogs
// app/_layout.* at layouts['']; whatever file fills that slot is what
// both entries render.
export const App = routes.layouts['']
