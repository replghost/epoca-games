"""Validated glTF 2.0 to Epoca's bounded GPU mesh format conversion."""

from __future__ import annotations

import base64
import binascii
import json
import math
import struct
from pathlib import Path, PurePosixPath
from urllib.parse import unquote, urlsplit

MAGIC = b"EPM1"
VERSION = 1
HEADER_BYTES = 80
VERTEX_STRIDE = 32
INDEX_BYTES = 4
MAX_SOURCE_BYTES = 64 * 1024 * 1024
MAX_OUTPUT_BYTES = 64 * 1024 * 1024
MAX_VERTICES = 1_000_000
MAX_INDICES = 3_000_000


class GltfError(Exception):
    pass


def convert(path: Path) -> bytes:
    path = path.resolve()
    document, embedded_buffer = _load_document(path)
    buffers = _load_buffers(path.parent, document, embedded_buffer)
    world, primitive = _selected_primitive(document)
    positions = _read_accessor(document, buffers, primitive.get("attributes", {}).get("POSITION"), 5126, "VEC3", "POSITION")
    normals = _read_accessor(document, buffers, primitive.get("attributes", {}).get("NORMAL"), 5126, "VEC3", "NORMAL")
    if "TEXCOORD_0" in primitive.get("attributes", {}):
        texture_coordinates = _read_accessor(
            document,
            buffers,
            primitive["attributes"]["TEXCOORD_0"],
            5126,
            "VEC2",
            "TEXCOORD_0",
        )
    else:
        texture_coordinates = [(0.0, 0.0)] * len(positions)
    if len(normals) != len(positions) or len(texture_coordinates) != len(positions):
        raise GltfError("POSITION, NORMAL, and TEXCOORD_0 accessors must have the same count")
    if not positions or len(positions) > MAX_VERTICES:
        raise GltfError(f"mesh must contain 1..{MAX_VERTICES} vertices")

    index_accessor = primitive.get("indices")
    indices = _read_indices(document, buffers, index_accessor)
    if not indices or len(indices) > MAX_INDICES or len(indices) % 3:
        raise GltfError(f"triangle index count must be a non-zero multiple of 3 up to {MAX_INDICES}")
    if max(indices) >= len(positions):
        raise GltfError("index accessor references a vertex outside POSITION")

    normal_matrix, determinant = _normal_matrix(world)
    transformed_positions = [_transform_point(world, position) for position in positions]
    transformed_normals = [_transform_normal(normal_matrix, normal) for normal in normals]
    if determinant < 0.0:
        for offset in range(0, len(indices), 3):
            indices[offset + 1], indices[offset + 2] = indices[offset + 2], indices[offset + 1]

    base_color = _base_color(document, primitive)
    bounds_min = tuple(min(position[axis] for position in transformed_positions) for axis in range(3))
    bounds_max = tuple(max(position[axis] for position in transformed_positions) for axis in range(3))
    if max(bounds_max[axis] - bounds_min[axis] for axis in range(3)) <= 1e-12:
        raise GltfError("mesh bounds are empty")

    vertex_bytes = bytearray()
    for position, normal, uv in zip(transformed_positions, transformed_normals, texture_coordinates):
        vertex_bytes.extend(struct.pack("<8f", *position, *normal, *uv))
    index_bytes = struct.pack(f"<{len(indices)}I", *indices)
    index_offset = HEADER_BYTES + len(vertex_bytes)
    output_size = index_offset + len(index_bytes)
    if output_size > MAX_OUTPUT_BYTES:
        raise GltfError(f"converted mesh exceeds {MAX_OUTPUT_BYTES} bytes")

    header = bytearray(HEADER_BYTES)
    struct.pack_into(
        "<4sHHIIIIII4f3fI3fI",
        header,
        0,
        MAGIC,
        VERSION,
        HEADER_BYTES,
        len(positions),
        len(indices),
        VERTEX_STRIDE,
        INDEX_BYTES,
        HEADER_BYTES,
        index_offset,
        *base_color,
        *bounds_min,
        0,
        *bounds_max,
        0,
    )
    return bytes(header + vertex_bytes + index_bytes)


def _load_document(path: Path) -> tuple[dict, bytes | None]:
    try:
        size = path.stat().st_size
        if size > MAX_SOURCE_BYTES:
            raise GltfError(f"source exceeds {MAX_SOURCE_BYTES} bytes: {path}")
        raw = path.read_bytes()
    except OSError as error:
        raise GltfError(f"could not read glTF source {path}: {error}") from error

    embedded_buffer = None
    if path.suffix.lower() == ".glb":
        document, embedded_buffer = _parse_glb(raw)
    elif path.suffix.lower() == ".gltf":
        document = _parse_json(raw, path)
    else:
        raise GltfError("glTF source must end in .gltf or .glb")
    if document.get("asset", {}).get("version") != "2.0":
        raise GltfError("glTF asset.version must be 2.0")
    if document.get("extensionsRequired") or document.get("extensionsUsed"):
        raise GltfError("glTF extensions are not supported")
    for unsupported in ("animations", "skins"):
        if document.get(unsupported):
            raise GltfError(f"glTF {unsupported} are not supported")
    return document, embedded_buffer


def _parse_json(raw: bytes, path: Path) -> dict:
    try:
        document = json.loads(raw)
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise GltfError(f"invalid glTF JSON {path}: {error}") from error
    if not isinstance(document, dict):
        raise GltfError("glTF root must be an object")
    return document


def _parse_glb(raw: bytes) -> tuple[dict, bytes | None]:
    if len(raw) < 12:
        raise GltfError("GLB header is truncated")
    magic, version, declared_length = struct.unpack_from("<4sII", raw)
    if magic != b"glTF" or version != 2 or declared_length != len(raw):
        raise GltfError("GLB header is invalid")
    offset = 12
    chunks: list[tuple[int, bytes]] = []
    while offset < len(raw):
        if len(raw) - offset < 8:
            raise GltfError("GLB chunk header is truncated")
        length, kind = struct.unpack_from("<II", raw, offset)
        offset += 8
        end = offset + length
        if end > len(raw):
            raise GltfError("GLB chunk is truncated")
        chunks.append((kind, raw[offset:end]))
        offset = end
    if not chunks or chunks[0][0] != 0x4E4F534A:
        raise GltfError("GLB must start with one JSON chunk")
    if len(chunks) > 2 or (len(chunks) == 2 and chunks[1][0] != 0x004E4942):
        raise GltfError("GLB may contain only JSON and BIN chunks")
    document = _parse_json(chunks[0][1].rstrip(b" \t\r\n\0"), Path("<GLB>"))
    return document, chunks[1][1] if len(chunks) == 2 else None


def _load_buffers(root: Path, document: dict, embedded_buffer: bytes | None) -> list[bytes]:
    descriptors = document.get("buffers")
    if not isinstance(descriptors, list) or not descriptors:
        raise GltfError("glTF must define at least one buffer")
    loaded = []
    total = 0
    for index, descriptor in enumerate(descriptors):
        if not isinstance(descriptor, dict) or not _is_nonnegative_int(descriptor.get("byteLength")):
            raise GltfError(f"buffer {index} has invalid byteLength")
        declared = descriptor["byteLength"]
        uri = descriptor.get("uri")
        if uri is None:
            if index != 0 or embedded_buffer is None:
                raise GltfError(f"buffer {index} has no URI or GLB BIN chunk")
            data = embedded_buffer
        elif not isinstance(uri, str) or not uri:
            raise GltfError(f"buffer {index} has invalid URI")
        elif uri.startswith("data:"):
            data = _decode_data_uri(uri, index)
        else:
            data = _read_relative_buffer(root, uri, index)
        if len(data) < declared or len(data) > declared + (3 if uri is None else 0):
            raise GltfError(f"buffer {index} byteLength does not match its data")
        data = data[:declared]
        total += len(data)
        if total > MAX_SOURCE_BYTES:
            raise GltfError(f"glTF buffers exceed {MAX_SOURCE_BYTES} bytes")
        loaded.append(data)
    return loaded


def _decode_data_uri(uri: str, index: int) -> bytes:
    prefix = "data:application/octet-stream;base64,"
    alternate = "data:application/gltf-buffer;base64,"
    if uri.startswith(prefix):
        encoded = uri[len(prefix) :]
    elif uri.startswith(alternate):
        encoded = uri[len(alternate) :]
    else:
        raise GltfError(f"buffer {index} uses an unsupported data URI")
    try:
        return base64.b64decode(encoded, validate=True)
    except (binascii.Error, ValueError) as error:
        raise GltfError(f"buffer {index} has invalid base64 data") from error


def _read_relative_buffer(root: Path, uri: str, index: int) -> bytes:
    parsed = urlsplit(uri)
    if parsed.scheme or parsed.netloc or parsed.query or parsed.fragment or "%2f" in uri.lower() or "%5c" in uri.lower():
        raise GltfError(f"buffer {index} URI must be a local relative path")
    decoded = unquote(parsed.path)
    relative = PurePosixPath(decoded)
    if relative.is_absolute() or not relative.parts or any(part in ("", ".", "..") for part in relative.parts):
        raise GltfError(f"buffer {index} URI escapes the source directory")
    candidate = root
    for part in relative.parts:
        candidate = candidate / part
        if candidate.is_symlink():
            raise GltfError(f"buffer {index} cannot use a symbolic link")
    resolved = candidate.resolve()
    try:
        resolved.relative_to(root.resolve())
    except ValueError as error:
        raise GltfError(f"buffer {index} URI escapes the source directory") from error
    try:
        if resolved.stat().st_size > MAX_SOURCE_BYTES:
            raise GltfError(f"buffer {index} exceeds {MAX_SOURCE_BYTES} bytes")
        return resolved.read_bytes()
    except OSError as error:
        raise GltfError(f"could not read buffer {index} at {candidate}: {error}") from error


def _selected_primitive(document: dict) -> tuple[tuple[float, ...], dict]:
    scenes = document.get("scenes")
    nodes = document.get("nodes")
    meshes = document.get("meshes")
    if not isinstance(scenes, list) or not scenes or not isinstance(nodes, list) or not isinstance(meshes, list):
        raise GltfError("glTF must define scenes, nodes, and meshes")
    scene_index = document.get("scene", 0)
    scene = _indexed(scenes, scene_index, "scene")
    roots = scene.get("nodes")
    if not isinstance(roots, list):
        raise GltfError("selected scene must define root nodes")

    selected: list[tuple[tuple[float, ...], dict]] = []

    def visit(node_index: object, parent: tuple[float, ...], stack: set[int]) -> None:
        if not _is_nonnegative_int(node_index) or node_index >= len(nodes):
            raise GltfError("scene references an invalid node")
        if node_index in stack:
            raise GltfError("node graph contains a cycle")
        node = nodes[node_index]
        if not isinstance(node, dict):
            raise GltfError(f"node {node_index} must be an object")
        if "camera" in node or "skin" in node or node.get("weights") is not None:
            raise GltfError(f"node {node_index} uses unsupported camera, skin, or morph data")
        world = _matrix_multiply(parent, _node_matrix(node, node_index))
        if "mesh" in node:
            mesh = _indexed(meshes, node["mesh"], f"node {node_index} mesh")
            primitives = mesh.get("primitives")
            if not isinstance(primitives, list) or len(primitives) != 1 or not isinstance(primitives[0], dict):
                raise GltfError("selected mesh must contain exactly one primitive")
            selected.append((world, primitives[0]))
        children = node.get("children", [])
        if not isinstance(children, list):
            raise GltfError(f"node {node_index} children must be an array")
        stack.add(node_index)
        for child in children:
            visit(child, world, stack)
        stack.remove(node_index)

    identity = (1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0)
    for root in roots:
        visit(root, identity, set())
    if len(selected) != 1:
        raise GltfError("selected scene must contain exactly one mesh node")
    world, primitive = selected[0]
    if primitive.get("mode", 4) != 4:
        raise GltfError("mesh primitive mode must be TRIANGLES")
    if primitive.get("targets"):
        raise GltfError("morph targets are not supported")
    attributes = primitive.get("attributes")
    if not isinstance(attributes, dict) or "POSITION" not in attributes or "NORMAL" not in attributes:
        raise GltfError("mesh primitive must provide POSITION and NORMAL")
    return world, primitive


def _node_matrix(node: dict, index: int) -> tuple[float, ...]:
    if "matrix" in node:
        if any(field in node for field in ("translation", "rotation", "scale")):
            raise GltfError(f"node {index} cannot combine matrix and TRS")
        matrix = node["matrix"]
        if not _finite_numbers(matrix, 16):
            raise GltfError(f"node {index} matrix is invalid")
        return tuple(float(value) for value in matrix)
    translation = node.get("translation", [0.0, 0.0, 0.0])
    rotation = node.get("rotation", [0.0, 0.0, 0.0, 1.0])
    scale = node.get("scale", [1.0, 1.0, 1.0])
    if not _finite_numbers(translation, 3) or not _finite_numbers(rotation, 4) or not _finite_numbers(scale, 3):
        raise GltfError(f"node {index} TRS is invalid")
    length = math.sqrt(sum(float(value) ** 2 for value in rotation))
    if length <= 1e-12:
        raise GltfError(f"node {index} rotation is invalid")
    x, y, z, w = (float(value) / length for value in rotation)
    sx, sy, sz = (float(value) for value in scale)
    rotation_scale = (
        (1 - 2 * (y * y + z * z)) * sx,
        (2 * (x * y + z * w)) * sx,
        (2 * (x * z - y * w)) * sx,
        0.0,
        (2 * (x * y - z * w)) * sy,
        (1 - 2 * (x * x + z * z)) * sy,
        (2 * (y * z + x * w)) * sy,
        0.0,
        (2 * (x * z + y * w)) * sz,
        (2 * (y * z - x * w)) * sz,
        (1 - 2 * (x * x + y * y)) * sz,
        0.0,
        float(translation[0]),
        float(translation[1]),
        float(translation[2]),
        1.0,
    )
    return rotation_scale


def _read_accessor(document: dict, buffers: list[bytes], index: object, component: int, shape: str, label: str) -> list[tuple[float, ...]]:
    accessor, data, offset, stride, count, components, component_format = _accessor_data(document, buffers, index, label)
    if accessor.get("componentType") != component or accessor.get("type") != shape or accessor.get("normalized", False):
        raise GltfError(f"{label} accessor must be non-normalized FLOAT {shape}")
    output = []
    value_format = "<" + component_format * components
    value_bytes = struct.calcsize(value_format)
    for item in range(count):
        values = struct.unpack_from(value_format, data, offset + item * stride)
        if not all(math.isfinite(value) for value in values):
            raise GltfError(f"{label} accessor contains non-finite values")
        output.append(tuple(float(value) for value in values))
    if value_bytes > stride:
        raise AssertionError("validated accessor stride")
    return output


def _read_indices(document: dict, buffers: list[bytes], index: object) -> list[int]:
    accessor, data, offset, stride, count, components, component_format = _accessor_data(document, buffers, index, "indices")
    if accessor.get("componentType") not in (5123, 5125) or accessor.get("type") != "SCALAR" or accessor.get("normalized", False):
        raise GltfError("indices accessor must be non-normalized UNSIGNED_SHORT or UNSIGNED_INT SCALAR")
    if components != 1:
        raise AssertionError("validated scalar accessor")
    return [struct.unpack_from("<" + component_format, data, offset + item * stride)[0] for item in range(count)]


def _accessor_data(document: dict, buffers: list[bytes], index: object, label: str) -> tuple[dict, bytes, int, int, int, int, str]:
    accessors = document.get("accessors")
    views = document.get("bufferViews")
    if not isinstance(accessors, list) or not isinstance(views, list):
        raise GltfError("glTF must define accessors and bufferViews")
    accessor = _indexed(accessors, index, f"{label} accessor")
    if accessor.get("sparse") is not None:
        raise GltfError(f"{label} sparse accessor is not supported")
    if not _is_nonnegative_int(accessor.get("count")):
        raise GltfError(f"{label} accessor count is invalid")
    count = accessor["count"]
    component_type = accessor.get("componentType")
    component_formats = {5123: (2, "H"), 5125: (4, "I"), 5126: (4, "f")}
    shape_components = {"SCALAR": 1, "VEC2": 2, "VEC3": 3}
    if component_type not in component_formats or accessor.get("type") not in shape_components:
        raise GltfError(f"{label} accessor type is unsupported")
    component_bytes, component_format = component_formats[component_type]
    components = shape_components[accessor["type"]]
    element_bytes = component_bytes * components
    view = _indexed(views, accessor.get("bufferView"), f"{label} bufferView")
    buffer_index = view.get("buffer")
    if not _is_nonnegative_int(buffer_index) or buffer_index >= len(buffers):
        raise GltfError(f"{label} bufferView references an invalid buffer")
    if not _is_nonnegative_int(view.get("byteLength")):
        raise GltfError(f"{label} bufferView byteLength is invalid")
    view_offset = view.get("byteOffset", 0)
    accessor_offset = accessor.get("byteOffset", 0)
    stride = view.get("byteStride", element_bytes)
    if not all(_is_nonnegative_int(value) for value in (view_offset, accessor_offset, stride)):
        raise GltfError(f"{label} accessor offsets are invalid")
    if stride < element_bytes or stride % component_bytes:
        raise GltfError(f"{label} accessor byteStride is invalid")
    start = view_offset + accessor_offset
    end = start + ((count - 1) * stride + element_bytes if count else 0)
    view_end = view_offset + view["byteLength"]
    data = buffers[buffer_index]
    if start % component_bytes or end > view_end or view_end > len(data):
        raise GltfError(f"{label} accessor exceeds its bufferView")
    return accessor, data, start, stride, count, components, component_format


def _base_color(document: dict, primitive: dict) -> tuple[float, float, float, float]:
    material_index = primitive.get("material")
    if material_index is None:
        return (1.0, 1.0, 1.0, 1.0)
    materials = document.get("materials")
    if not isinstance(materials, list):
        raise GltfError("primitive references a material but materials are missing")
    material = _indexed(materials, material_index, "material")
    if material.get("alphaMode", "OPAQUE") != "OPAQUE" or material.get("doubleSided", False):
        raise GltfError("material must be opaque and single-sided")
    if material.get("normalTexture") or material.get("occlusionTexture") or material.get("emissiveTexture"):
        raise GltfError("material textures are not supported")
    if material.get("emissiveFactor", [0.0, 0.0, 0.0]) != [0.0, 0.0, 0.0]:
        raise GltfError("emissive materials are not supported")
    pbr = material.get("pbrMetallicRoughness", {})
    if not isinstance(pbr, dict) or pbr.get("baseColorTexture") or pbr.get("metallicRoughnessTexture"):
        raise GltfError("material textures are not supported")
    factor = pbr.get("baseColorFactor", [1.0, 1.0, 1.0, 1.0])
    if not _finite_numbers(factor, 4) or any(not 0.0 <= float(value) <= 1.0 for value in factor):
        raise GltfError("material baseColorFactor must contain four values in 0..1")
    return tuple(float(value) for value in factor)


def _matrix_multiply(left: tuple[float, ...], right: tuple[float, ...]) -> tuple[float, ...]:
    return tuple(
        sum(left[index * 4 + row] * right[column * 4 + index] for index in range(4))
        for column in range(4)
        for row in range(4)
    )


def _transform_point(matrix: tuple[float, ...], point: tuple[float, ...]) -> tuple[float, float, float]:
    output = tuple(
        matrix[row] * point[0] + matrix[4 + row] * point[1] + matrix[8 + row] * point[2] + matrix[12 + row]
        for row in range(3)
    )
    if not all(math.isfinite(value) for value in output):
        raise GltfError("node transform produces a non-finite position")
    return output


def _normal_matrix(matrix: tuple[float, ...]) -> tuple[tuple[float, ...], float]:
    a00, a01, a02 = matrix[0], matrix[4], matrix[8]
    a10, a11, a12 = matrix[1], matrix[5], matrix[9]
    a20, a21, a22 = matrix[2], matrix[6], matrix[10]
    determinant = a00 * (a11 * a22 - a12 * a21) - a01 * (a10 * a22 - a12 * a20) + a02 * (a10 * a21 - a11 * a20)
    if not math.isfinite(determinant) or abs(determinant) <= 1e-12:
        raise GltfError("mesh node transform is singular")
    inverse = 1.0 / determinant
    cofactors = (
        a11 * a22 - a12 * a21,
        a12 * a20 - a10 * a22,
        a10 * a21 - a11 * a20,
        a02 * a21 - a01 * a22,
        a00 * a22 - a02 * a20,
        a01 * a20 - a00 * a21,
        a01 * a12 - a02 * a11,
        a02 * a10 - a00 * a12,
        a00 * a11 - a01 * a10,
    )
    return tuple(value * inverse for value in cofactors), determinant


def _transform_normal(matrix: tuple[float, ...], normal: tuple[float, ...]) -> tuple[float, float, float]:
    transformed = tuple(
        matrix[row * 3] * normal[0] + matrix[row * 3 + 1] * normal[1] + matrix[row * 3 + 2] * normal[2]
        for row in range(3)
    )
    length = math.sqrt(sum(value * value for value in transformed))
    if not math.isfinite(length) or length <= 1e-12:
        raise GltfError("normal is zero-length after node transform")
    return tuple(value / length for value in transformed)


def _indexed(values: list, index: object, label: str) -> dict:
    if not _is_nonnegative_int(index) or index >= len(values) or not isinstance(values[index], dict):
        raise GltfError(f"{label} index is invalid")
    return values[index]


def _is_nonnegative_int(value: object) -> bool:
    return isinstance(value, int) and not isinstance(value, bool) and value >= 0


def _finite_numbers(value: object, length: int) -> bool:
    return isinstance(value, list) and len(value) == length and all(
        isinstance(item, (int, float)) and not isinstance(item, bool) and math.isfinite(item) for item in value
    )
