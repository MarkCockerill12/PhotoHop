# Saving & exporting

## Projects

- **File > Save (Ctrl+S)** saves a `.phop` project file. Next to it, a folder `<name>.assets` holds your masks, paint, AI results and generated files — keep the two together when you move or share a project.
- **Save as… (Ctrl+Shift+S)** saves a copy under a new name.
- **Open… (Ctrl+O)** opens a project. You can also drop a `.phop` file onto the window.
- Media (your photos and videos) is linked, not copied: paths are stored relative to the project, so move the media along with it. A **red** layer name means a file wasn't found.

## Exporting an image

**File > Export…**, under **Current frame as image**, choose a format and click **Export image…**:

| Format | Use it for |
|---|---|
| PNG 8-bit | sharing, web, most uses (lossless) |
| PNG 16-bit | further editing without banding |
| JPEG | small files for phones and social media |
| OpenEXR (linear float) | professional compositing / VFX |

The image is the frame at the playhead, at full canvas resolution.

## Exporting video

Under **Timeline as video**, choose a codec and click **Export video…**:

| Codec | Use it for |
|---|---|
| H.264 MP4 (phone-ready) | plays everywhere |
| H.265/HEVC MP4 (smaller) | smaller files, modern devices |
| ProRes 4444 MOV (with alpha) | editing software; keeps transparency |

**Include audio from the first video layer** keeps your clip's original sound. A progress bar shows in the status bar; **Cancel export** stops it. Export always uses full quality, whatever the preview quality is set to.

## Canvas settings

Select nothing (click empty space in the Layers list) to see the **Canvas**: size, frame rate, duration and background colour (transparent by default, which PNG and ProRes keep).
