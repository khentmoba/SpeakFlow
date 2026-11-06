# Model catalog browser

Screenshots of `ModelBrowser` in Settings → ASR Engine, captured from the real
component with the real generated catalog and the real stylesheets.

They were rendered in a temporary Vite page that mounts the component with
`engine="parakeet-unified-en-0.6b"` and two installed models. That page is not
part of the repository. The packaged Tauri window was not used, so the captures
show the component and its layout, not the application chrome or live IPC.

- `all-languages.png`: the default view, ranked by English grade.
- `french.png`: the same list filtered to French, ranked by the French grade.

The grades, error rates and speeds come from Handy's published benchmark. The
list says so under the models.
