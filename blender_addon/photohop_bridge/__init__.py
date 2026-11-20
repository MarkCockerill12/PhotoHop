# SPDX-License-Identifier: GPL-3.0-or-later
"""PhotoHop Bridge: build a Blender scene from a PhotoHop bundle and render props + shadows back.

The scene: a camera matching the PhotoHop canvas, the subject as a camera-facing card that casts
shadows and hides (holds out) anything behind it but is itself invisible, a shadow-catcher floor at
the subject's feet, and a sun matching PhotoHop's Relight pad. Add props, then Render for PhotoHop.

Headless use (testing / batch):
    blender -b --factory-startup --python __init__.py -- BUNDLE_DIR [--render] [--test-prop]
"""

import json
import math
import os
import struct
import sys

import bpy
from bpy.props import StringProperty
from bpy_extras.io_utils import ImportHelper
from mathutils import Matrix, Vector

bl_info = {
    "name": "PhotoHop Bridge",
    "author": "PhotoHop",
    "version": (0, 2, 0),
    "blender": (4, 2, 0),
    "location": "File > Import > PhotoHop bundle; Render > Render for PhotoHop",
    "description": "Round-trip PhotoHop composites through Blender (props, shadows, reflections)",
    "category": "Import-Export",
}

DISTANCE = 5.0  # metres from camera to the subject card


def _load_manifest(path):
    folder = path if os.path.isdir(path) else os.path.dirname(path)
    with open(os.path.join(folder, "manifest.json"), encoding="utf8") as f:
        return folder, json.load(f)


def _sequence_image(folder, pattern, frames):
    """Load a printf-pattern image sequence as a Blender image (SEQUENCE source)."""
    first = os.path.join(folder, pattern.replace("%04d", "0001"))
    img = bpy.data.images.load(first, check_existing=True)
    if frames > 1:
        img.source = "SEQUENCE"
    return img


def _card_material(img, frames):
    mat = bpy.data.materials.new("PhotoHop subject card")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    tex = nt.nodes.new("ShaderNodeTexImage")
    tex.image = img
    tex.interpolation = "Linear"
    if frames > 1:
        tex.image_user.frame_duration = frames
        tex.image_user.frame_start = 1
        tex.image_user.use_auto_refresh = True
    transparent = nt.nodes.new("ShaderNodeBsdfTransparent")
    holdout = nt.nodes.new("ShaderNodeHoldout")
    diffuse = nt.nodes.new("ShaderNodeBsdfDiffuse")
    diffuse.inputs["Color"].default_value = (0.0, 0.0, 0.0, 1.0)
    camera_mix = nt.nodes.new("ShaderNodeMixShader")  # camera rays: hole where the subject is
    shadow_mix = nt.nodes.new("ShaderNodeMixShader")  # shadow rays: opaque where the subject is
    path = nt.nodes.new("ShaderNodeLightPath")
    final = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(tex.outputs["Alpha"], camera_mix.inputs["Fac"])
    nt.links.new(transparent.outputs[0], camera_mix.inputs[1])
    nt.links.new(holdout.outputs[0], camera_mix.inputs[2])
    nt.links.new(tex.outputs["Alpha"], shadow_mix.inputs["Fac"])
    nt.links.new(transparent.outputs[0], shadow_mix.inputs[1])
    nt.links.new(diffuse.outputs[0], shadow_mix.inputs[2])
    nt.links.new(path.outputs["Is Shadow Ray"], final.inputs["Fac"])
    nt.links.new(camera_mix.outputs[0], final.inputs[1])
    nt.links.new(shadow_mix.outputs[0], final.inputs[2])
    nt.links.new(final.outputs[0], out.inputs["Surface"])
    if hasattr(mat, "surface_render_method"):
        mat.surface_render_method = "BLENDED"
    return mat


def _collection(name):
    col = bpy.data.collections.get(name) or bpy.data.collections.new(name)
    if col.name not in bpy.context.scene.collection.children:
        bpy.context.scene.collection.children.link(col)
    return col


def build_scene(bundle):
    folder, m = _load_manifest(bundle)
    scene = bpy.context.scene
    w, h, fps, frames = int(m["width"]), int(m["height"]), float(m["fps"]), int(m["frames"])

    try:
        bpy.ops.preferences.addon_enable(module="cycles")
    except Exception as e:  # noqa: BLE001 - already enabled / built in
        print("PhotoHop Bridge: could not enable Cycles add-on:", e)
    scene.render.engine = "CYCLES"
    scene.cycles.samples = 64
    scene.render.film_transparent = True
    scene.render.resolution_x, scene.render.resolution_y = w, h
    scene.render.resolution_percentage = 100
    scene.render.fps = max(1, round(fps))
    scene.render.fps_base = scene.render.fps / fps
    scene.frame_start, scene.frame_end = 1, frames
    ims = scene.render.image_settings
    if hasattr(ims, "media_type"):
        ims.media_type = "IMAGE"
    ims.file_format = "PNG"
    ims.color_mode = "RGBA"
    scene.render.filepath = os.path.join(folder, m.get("render", "render/render_"))
    scene.render.use_file_extension = True

    col = _collection("PhotoHop")
    _collection("Props")

    # Camera looking along +Y, horizon at the vertical centre of the frame.
    fov = math.radians(float(m.get("fov_deg", 50.0)))
    cam_data = bpy.data.cameras.new("PhotoHop camera")
    cam_data.sensor_fit = "AUTO"
    cam_data.angle = fov
    cam = bpy.data.objects.new("PhotoHop camera", cam_data)
    cam.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    col.objects.link(cam)
    scene.camera = cam

    # Size of the frame at the card distance (sensor fit AUTO = the larger side gets the angle).
    big = 2.0 * DISTANCE * math.tan(fov / 2.0)
    card_w, card_h = (big, big * h / w) if w >= h else (big * w / h, big)

    # Subject card filling the frame.
    bpy.ops.mesh.primitive_plane_add(size=1.0)
    card = bpy.context.active_object
    card.name = "PhotoHop subject"
    card.scale = (card_w, card_h, 1.0)
    card.rotation_euler = (math.radians(90.0), 0.0, 0.0)
    card.location = (0.0, DISTANCE, 0.0)
    for c in list(card.users_collection):
        c.objects.unlink(card)
    col.objects.link(card)
    subject = _sequence_image(folder, m["subject"], frames)
    card.data.materials.append(_card_material(subject, frames))
    if hasattr(card, "visible_shadow"):
        card.visible_shadow = True

    # Shadow-catcher floor at the feet line.
    _fx, fy = m.get("feet_px", [w / 2, h])
    ground_z = (0.5 - float(fy) / h) * card_h
    bpy.ops.mesh.primitive_plane_add(size=200.0, location=(0.0, DISTANCE + 90.0, ground_z))
    floor = bpy.context.active_object
    floor.name = "PhotoHop shadow catcher"
    floor.is_shadow_catcher = True
    for c in list(floor.users_collection):
        c.objects.unlink(floor)
    col.objects.link(floor)

    # Camera background (viewport only) for lining props up with the scene.
    bg = _sequence_image(folder, m["background"], frames)
    cam_data.show_background_images = True
    bgi = cam_data.background_images.new()
    bgi.image = bg
    bgi.alpha = 1.0
    if frames > 1:
        bgi.image_user.frame_duration = frames
        bgi.image_user.use_auto_refresh = True

    # Sun from PhotoHop's light pad: camera space (x right, y up, z towards viewer) → world.
    light = m.get("light", {})
    az, el = math.radians(float(light.get("azimuth", 45.0))), math.radians(float(light.get("elevation", 35.0)))
    d_cam = Vector((math.cos(el) * math.cos(az), math.cos(el) * math.sin(az), math.sin(el)))
    to_light = Vector((d_cam.x, -d_cam.z, d_cam.y)).normalized()
    sun_data = bpy.data.lights.new("PhotoHop sun", type="SUN")
    sun_data.energy = 3.0 * float(light.get("intensity", 1.0))
    sun_data.color = tuple(light.get("color", [1.0, 1.0, 1.0]))[:3]
    sun_data.angle = math.radians(2.0)
    sun = bpy.data.objects.new("PhotoHop sun", sun_data)
    sun.rotation_euler = to_light.to_track_quat("Z", "Y").to_euler()
    col.objects.link(sun)

    if m.get("camera"):
        _apply_camera_solve(folder, m, scene, cam, cam_data, card, floor, sun, (w, h))
    if m.get("body"):
        _apply_body(folder, m, scene, cam, cam_data, card, floor, col, (w, h))

    world = scene.world or bpy.data.worlds.new("World")
    scene.world = world
    world.use_nodes = True
    bgnode = world.node_tree.nodes.get("Background")
    if bgnode:
        bgnode.inputs["Color"].default_value = (0.35, 0.35, 0.38, 1.0)
        bgnode.inputs["Strength"].default_value = 0.6
    scene["photohop_bundle"] = folder
    return folder


# OpenCV world (x right, y down, z forward) → Blender world (x right, y forward, z up).
_CV_TO_BL = Matrix(((1, 0, 0, 0), (0, 0, 1, 0), (0, -1, 0, 0), (0, 0, 0, 1)))
# OpenCV camera axes → Blender camera axes (Blender cameras look down -Z with +Y up).
_CAM_FLIP = Matrix(((1, 0, 0, 0), (0, -1, 0, 0), (0, 0, -1, 0), (0, 0, 0, 1)))


def _apply_camera_solve(folder, m, scene, cam, cam_data, card, floor, sun, size):
    """Animate the camera from camera.json, put the floor on the solved ground, and keep the subject
    card glued to the camera at the subject's solved distance."""
    with open(os.path.join(folder, m["camera"]), encoding="utf8") as f:
        solve = json.load(f)
    w, h = size
    frames = solve["camera_to_world"]
    k = solve["intrinsics"]
    sx = w / float(solve["width"])

    # Align the solved ground with Blender's floor (z = 0) when it really points up.
    world = _CV_TO_BL.copy()
    ground = solve.get("ground")
    ground_ok = bool(ground) and ground["normal"][1] < -0.6
    if ground_ok:
        n = (_CV_TO_BL.to_3x3() @ Vector(ground["normal"])).normalized()
        p = _CV_TO_BL @ Vector(ground["point"])
        rot = n.rotation_difference(Vector((0.0, 0.0, 1.0))).to_matrix().to_4x4()
        lift = Matrix.Translation((0.0, 0.0, -(rot @ p).z))
        world = lift @ rot @ _CV_TO_BL

    # Lens from the solved focal length (36 mm sensor across the width).
    cam_data.sensor_fit = "HORIZONTAL"
    cam_data.sensor_width = 36.0
    cam_data.lens = float(k["fx"]) * sx / w * 36.0
    cam.rotation_mode = "QUATERNION"
    cam.animation_data_clear()
    for i, c2w in enumerate(frames[: scene.frame_end]):
        cam.matrix_world = world @ Matrix(c2w) @ _CAM_FLIP
        cam.keyframe_insert("location", frame=i + 1)
        cam.keyframe_insert("rotation_quaternion", frame=i + 1)
    scene.frame_set(1)
    view_layer = bpy.context.view_layer
    view_layer.update()

    # Subject distance: where the ray through the feet meets the ground (else keep 5 m).
    dist = DISTANCE
    if ground_ok:
        fx_px, fy_px = m.get("feet_px", [w / 2, h])
        ray_cam = Vector(((fx_px / sx - k["cx"]) / k["fx"], (fy_px / sx - k["cy"]) / k["fy"], 1.0))
        c2w0 = world @ Matrix(frames[0])
        origin = c2w0.to_translation()
        direction = (c2w0.to_3x3() @ ray_cam).normalized()
        if abs(direction.z) > 1e-4:
            t = -origin.z / direction.z
            if t > 0:
                dist = t / ray_cam.length * ray_cam.z
    # Card glued to the camera, filling the frame at that distance.
    fov_x = 2.0 * math.atan(w / (2.0 * float(k["fx"]) * sx))
    card_w = 2.0 * dist * math.tan(fov_x / 2.0)
    card_h = card_w * h / w
    card.parent = cam
    card.matrix_parent_inverse = Matrix.Identity(4)
    card.location = (0.0, 0.0, -dist)
    card.rotation_euler = (0.0, 0.0, 0.0)
    card.scale = (card_w, card_h, 1.0)
    if ground_ok:
        floor.location = (cam.matrix_world.to_translation().x, cam.matrix_world.to_translation().y, 0.0)
    else:
        # No trustworthy ground: put the floor through the feet at the card distance, using the
        # solved lens (the static 50° guess no longer applies).
        fx_px, fy_px = m.get("feet_px", [w / 2, h])
        feet_cam = Vector(((fx_px / sx - k["cx"]) / k["fx"] * dist, (fy_px / sx - k["cy"]) / k["fy"] * dist, dist))
        feet_world = world @ Matrix(frames[0]) @ feet_cam
        cam0 = cam.matrix_world.to_translation()
        floor.location = (cam0.x, cam0.y, feet_world.z)
    # The sun was set in camera-0 space; carry it into the solved world.
    sun.matrix_world = (world @ Matrix(frames[0]) @ _CAM_FLIP).to_quaternion().to_matrix().to_4x4() @ sun.matrix_world
    scene["photohop_camera_solved"] = True
    scene["photohop_ground_solved"] = ground_ok


def _body_material():
    """Holdout for the camera (the real footage shows through and the body hides props behind it),
    solid for shadow rays (the body casts shadows on props and the floor)."""
    mat = bpy.data.materials.new("PhotoHop body")
    mat.use_nodes = True
    nt = mat.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    holdout = nt.nodes.new("ShaderNodeHoldout")
    diffuse = nt.nodes.new("ShaderNodeBsdfDiffuse")
    diffuse.inputs["Color"].default_value = (0.0, 0.0, 0.0, 1.0)
    path = nt.nodes.new("ShaderNodeLightPath")
    mix = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(path.outputs["Is Shadow Ray"], mix.inputs["Fac"])
    nt.links.new(holdout.outputs[0], mix.inputs[1])
    nt.links.new(diffuse.outputs[0], mix.inputs[2])
    nt.links.new(mix.outputs[0], out.inputs["Surface"])
    return mat


def _read_obj(path):
    verts, faces = [], []
    with open(path, encoding="utf8") as f:
        for line in f:
            if line.startswith("v "):
                verts.append(tuple(map(float, line.split()[1:4])))
            elif line.startswith("f "):
                faces.append(tuple(int(t.split("/")[0]) - 1 for t in line.split()[1:]))
    return verts, faces


def _pc2_depths(path):
    """Per-frame (min, max) camera depth of the body, read from the PC2 cache."""
    import numpy as np

    with open(path, "rb") as f:
        f.read(12)
        _ver, n_verts, _start, _rate, n_frames = struct.unpack("<iiffi", f.read(20))
        data = np.fromfile(f, dtype="<f4", count=n_frames * n_verts * 3).reshape(n_frames, n_verts, 3)
    z = data[:, :, 2]
    return z.min(1), z.max(1), data[0]


def _apply_body(folder, m, scene, cam, cam_data, card, floor, col, size):
    """Animated 3D body (SAM 3D Body via PhotoHop) glued to the camera in its own camera space:
    occludes props behind you, casts real shadows, and gives hand empties to parent props to."""
    with open(os.path.join(folder, m["body"]), encoding="utf8") as f:
        body = json.load(f)
    w, h = size
    k = body["intrinsics"]
    sx = w / float(body["width"])
    fx = float(k["fx"]) * sx
    if not scene.get("photohop_camera_solved"):
        # The body was fitted with this lens; the static camera must use it too.
        cam_data.sensor_fit = "HORIZONTAL"
        cam_data.sensor_width = 36.0
        cam_data.lens = fx / w * 36.0

    stem = os.path.splitext(os.path.join(folder, m["body"]))[0]  # body.json → body.obj / body.pc2
    verts, faces = _read_obj(stem + ".obj")
    mesh = bpy.data.meshes.new("PhotoHop body")
    mesh.from_pydata(verts, [], faces)
    mesh.update()
    for poly in mesh.polygons:
        poly.use_smooth = True
    obj = bpy.data.objects.new("PhotoHop body", mesh)
    col.objects.link(obj)
    obj.data.materials.append(_body_material())
    obj.parent = cam
    obj.matrix_parent_inverse = Matrix.Identity(4)
    obj.matrix_basis = _CAM_FLIP.copy()
    mod = obj.modifiers.new("PhotoHop motion", "MESH_CACHE")
    mod.cache_format = "PC2"
    mod.filepath = stem + ".pc2"
    mod.frame_start = 1.0
    mod.forward_axis, mod.up_axis = "POS_Y", "POS_Z"

    # Card (pixel-accurate holdout of the footage) sits just behind the body each frame, so props
    # in front of any part of you show, and the body mesh handles props between your limbs.
    _near, far, first = _pc2_depths(mod.filepath)
    card.parent = cam
    card.matrix_parent_inverse = Matrix.Identity(4)
    card.rotation_euler = (0.0, 0.0, 0.0)
    for i in range(min(len(far), scene.frame_end)):
        d = float(far[i]) + 0.05
        card.location = (0.0, 0.0, -d)
        card.scale = (d * w / fx, d * w / fx * h / w, 1.0)
        card.keyframe_insert("location", frame=i + 1)
        card.keyframe_insert("scale", frame=i + 1)
    if hasattr(card, "visible_shadow"):
        card.visible_shadow = False  # the body casts the shadows now

    # Floor through your lowest point on frame 1 unless the camera solve found the real ground.
    scene.frame_set(1)
    bpy.context.view_layer.update()
    if not scene.get("photohop_ground_solved"):
        lowest = max(first, key=lambda v: v[1])  # largest y = lowest in OpenCV camera space
        p = cam.matrix_world @ _CAM_FLIP @ Vector(lowest)
        floor.location = (p.x, p.y, p.z)

    # Hand empties: X along the fingers, Z out of the back of the hand. Parent props to these.
    for side, mats in body.get("hands", {}).items():
        e = bpy.data.objects.new(f"PhotoHop {side} hand", None)
        e.empty_display_type = "ARROWS"
        e.empty_display_size = 0.1
        col.objects.link(e)
        e.parent = cam
        e.matrix_parent_inverse = Matrix.Identity(4)
        e.rotation_mode = "QUATERNION"
        for i, mm in enumerate(mats[: scene.frame_end]):
            e.matrix_basis = _CAM_FLIP @ Matrix(mm)
            e.keyframe_insert("location", frame=i + 1)
            e.keyframe_insert("rotation_quaternion", frame=i + 1)
    scene.frame_set(1)
    scene["photohop_body"] = True


class PHOTOHOP_OT_import_bundle(bpy.types.Operator, ImportHelper):
    """Build a scene from a PhotoHop bundle (pick its manifest.json)"""

    bl_idname = "photohop.import_bundle"
    bl_label = "Import PhotoHop bundle"
    filename_ext = ".json"
    filter_glob: StringProperty(default="manifest.json", options={"HIDDEN"})

    def execute(self, context):
        folder = build_scene(self.filepath)
        self.report({"INFO"}, f"PhotoHop scene ready ({folder}). Add props to the 'Props' collection.")
        return {"FINISHED"}


class PHOTOHOP_OT_render(bpy.types.Operator):
    """Render props and shadows into the bundle's render folder"""

    bl_idname = "photohop.render"
    bl_label = "Render for PhotoHop"

    def execute(self, context):
        if "photohop_bundle" not in context.scene:
            self.report({"ERROR"}, "Import a PhotoHop bundle first")
            return {"CANCELLED"}
        bpy.ops.render.render(animation=True, write_still=False)
        self.report({"INFO"}, "Done. In PhotoHop: File > Import Blender render…")
        return {"FINISHED"}


def _menu_import(self, _context):
    self.layout.operator(PHOTOHOP_OT_import_bundle.bl_idname, text="PhotoHop bundle (manifest.json)")


def _menu_render(self, _context):
    self.layout.operator(PHOTOHOP_OT_render.bl_idname)


CLASSES = (PHOTOHOP_OT_import_bundle, PHOTOHOP_OT_render)


def register():
    for c in CLASSES:
        bpy.utils.register_class(c)
    bpy.types.TOPBAR_MT_file_import.append(_menu_import)
    bpy.types.TOPBAR_MT_render.append(_menu_render)


def unregister():
    bpy.types.TOPBAR_MT_render.remove(_menu_render)
    bpy.types.TOPBAR_MT_file_import.remove(_menu_import)
    for c in reversed(CLASSES):
        bpy.utils.unregister_class(c)


def _cli():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if not argv:
        return
    bpy.ops.wm.read_factory_settings(use_empty=True)
    folder = build_scene(argv[0])
    if "--test-prop" in argv:
        # A bright cube standing on the floor in front of the frame-1 camera, to check shadows,
        # holdout and (with a camera solve) that it stays put in the world as the camera moves.
        sc = bpy.context.scene
        sc.frame_set(1)
        cam = sc.camera
        floor_z = sc.objects["PhotoHop shadow catcher"].location.z
        fwd = cam.matrix_world.to_3x3() @ Vector((0.0, 0.0, -1.0))
        right = cam.matrix_world.to_3x3() @ Vector((1.0, 0.0, 0.0))
        fwd.z, right.z = 0.0, 0.0
        card = sc.objects["PhotoHop subject"]
        dist = abs(card.location.z) if card.parent else DISTANCE
        if "--prop-depth" in argv:
            dist = float(argv[argv.index("--prop-depth") + 1]) / 0.8
        side = float(argv[argv.index("--prop-side") + 1]) if "--prop-side" in argv else -0.15
        pos = cam.matrix_world.to_translation() + fwd.normalized() * dist * 0.8 + right.normalized() * dist * side
        z = cam.matrix_world.to_translation().z if "--prop-depth" in argv else floor_z + 0.06 * dist
        bpy.ops.mesh.primitive_cube_add(size=0.12 * dist, location=(pos.x, pos.y, z))
        cube = bpy.context.active_object
        mat = bpy.data.materials.new("test")
        mat.use_nodes = True
        mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.9, 0.2, 0.1, 1.0)
        cube.data.materials.append(mat)
    if "--test-hand-prop" in argv and "PhotoHop right hand" in bpy.context.scene.objects:
        # A bright 'gun' (30 cm bar) held in the right hand: checks hand tracking + body occlusion.
        hand = bpy.context.scene.objects["PhotoHop right hand"]
        bpy.ops.mesh.primitive_cube_add(size=1.0)
        bar = bpy.context.active_object
        bar.name = "Test hand prop"
        bar.scale = (0.3, 0.04, 0.06)
        bar.parent = hand
        bar.matrix_parent_inverse = Matrix.Identity(4)
        bar.location = (0.1, 0.0, 0.0)
        mat = bpy.data.materials.new("Test prop orange")
        mat.diffuse_color = (1.0, 0.35, 0.05, 1.0)
        mat.use_nodes = True
        mat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (1.0, 0.35, 0.05, 1.0)
        bar.data.materials.append(mat)

    if "--render" in argv:
        sc = bpy.context.scene
        sc.cycles.samples = 32
        if "--frames" in argv:
            # Render only the listed frames (comma separated), e.g. --frames 1,99
            base = sc.render.filepath
            for f in (int(x) for x in argv[argv.index("--frames") + 1].split(",")):
                sc.frame_set(f)
                sc.render.filepath = f"{base}{f:04d}"
                bpy.ops.render.render(write_still=True)
            sc.render.filepath = base
        else:
            if "--first-frame" in argv:
                sc.frame_end = sc.frame_start
            bpy.ops.render.render(animation=True)
    print("PHOTOHOP_BRIDGE_OK", folder)


if __name__ == "__main__":
    _cli()
