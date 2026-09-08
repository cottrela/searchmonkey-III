# Searchmonkey III 0.4.1

This bugfix release improves Linux compatibility and makes advanced search limits and binary-file handling clearer.

## Fixes

- Fixed an AppImage startup crash on newer Wayland and Mesa systems that reported `Could not create surfaceless EGL display: EGL_BAD_ALLOC`. The AppImage now uses the host system's compatible Wayland libraries instead of bundling older copies.
- Fixed window controls on KDE Wayland not following the configured close, minimise, and maximise button layout. Searchmonkey now uses XWayland on KDE Wayland by default; users can still select a backend explicitly with `GDK_BACKEND`.

## Improvements

- Renamed **Max matches** to **Maximum total matches**, clarifying that the limit applies across all searched files rather than separately to each file.
- Split binary-file handling into two independent options:
  - **Search supported document formats** controls whether Searchmonkey includes documents handled by installed plugins, such as PDF, Office documents, or images with OCR.
  - **Skip unsupported binary files** controls whether unrecognised binary files are ignored by the plain-text search engine.

Thanks to the Searchmonkey community for the detailed Manjaro KDE and Wayland diagnostics that helped identify these issues.
