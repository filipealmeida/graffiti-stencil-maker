# Node parameter reference

Every parameter `--set NODE.PARAM=VALUE` accepts. Generated with `stencil --list-params --params-format markdown`; regenerate it when a node changes.

### Image (`source`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `image_id` | Image | text | `""` | text |
| `resolution` | Working resolution (px) | int | `1000.0` | 20.0 to 2000.0 |

### Text (`text`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `text` | Text | string | `"STENCIL"` | text |
| `font` | Font | choice | `"sans bold"` | sans bold \| serif bold |
| `size` | Letter height (px) | int | `160.0` | 20.0 to 600.0 |
| `line_spacing` | Line spacing (x) | float | `1.0` | 0.7 to 2.0 |
| `padding` | Padding (px) | int | `20.0` | 0.0 to 200.0 |

### Crop & rotate (`transform`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `crop_left` | Crop left (%) | float | `0.0` | 0.0 to 90.0 |
| `crop_top` | Crop top (%) | float | `0.0` | 0.0 to 90.0 |
| `crop_right` | Crop right (%) | float | `0.0` | 0.0 to 90.0 |
| `crop_bottom` | Crop bottom (%) | float | `0.0` | 0.0 to 90.0 |
| `turn` | Turn (deg) | choice | `"0"` | 0 \| 90 \| 180 \| 270 |
| `angle` | Fine rotation (deg) | float | `0.0` | -45.0 to 45.0 |
| `flip_h` | Flip horizontally | bool | `false` | true \| false |
| `flip_v` | Flip vertically | bool | `false` | true \| false |

### Threshold (`threshold`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `threshold` | Threshold | int | `256.0` | 0.0 to 256.0 |
| `invert` | Invert | bool | `false` | true \| false |

### Reduce colours (`reduce`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `colours` | Colours | int | `3.0` | 2.0 to 10.0 |
| `smooth` | Pre-blur (px) | float | `0.0` | 0.0 to 6.0 |

### Split colours (`split_colours`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `invert` | Invert order | bool | `false` | true \| false |
| `colours` | Most colours to keep | int | `10.0` | 2.0 to 10.0 |
| `min_area` | Drop specks smaller than (% of image) | float | `0.0` | 0.0 to 5.0 |

### Apply tone patterns (`tone_patterns`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `plate_width_mm` | Plate width (mm) | int | `100.0` | 20.0 to 2000.0 |
| `invert` | Invert | bool | `false` | true \| false |
| `kind` | Default pattern type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker |
| `spacing_mm` | Default pattern spacing (mm) | float | `2.5` | 0.0 to 20.0 |
| `angle` | Default pattern angle (deg) | int | `45.0` | 0.0 to 90.0 |
| `kind_1` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_1` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_1` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_2` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_2` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_2` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_3` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_3` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_3` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_4` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_4` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_4` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_5` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_5` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_5` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_6` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_6` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_6` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_7` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_7` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_7` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |
| `kind_8` | Type | choice | `"auto"` | auto \| lines \| dots \| crosshatch \| checker \| solid \| empty |
| `spacing_8` | Spacing (mm) | float | `0.0` | 0.0 to 20.0 |
| `cover_8` | Coverage (%) | float | `0.0` | 0.0 to 95.0 |

### Smooth mask (`smooth`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `radius` | Smoothing (px) | float | `1.5` | 0.0 to 10.0 |
| `supersample` | Supersample (x) | int | `2.0` | 1.0 to 4.0 |

### Reshape paint (`morphology`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `op` | Operation | choice | `"open"` | erode \| dilate \| open \| close \| gradient \| white_hat \| black_hat \| thin \| skeleton \| hit_or_miss |
| `kernel` | Kernel shape | choice | `"disc"` | square \| disc \| diamond \| cross \| line \| ring |
| `radius` | Kernel radius (px) | int | `2.0` | 1.0 to 30.0 |
| `angle` | Line angle (deg) | float | `0.0` | 0.0 to 179.0 |
| `iterations` | Iterations | int | `1.0` | 0.0 to 10.0 |
| `pattern` | Hit-or-miss pattern | choice | `"endpoints"` | isolated \| endpoints \| corners \| junctions \| custom |
| `custom` | Custom 3x3 pattern | string | `"x00 011 x1x"` | text |
| `rotate` | Also match 90° rotations | bool | `true` | true \| false |

### Trace (`trace`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `smooth` | Smoothing (px) | float | `1.5` | 0.0 to 8.0 |
| `corner_deg` | Sharp corner angle (deg) | float | `60.0` | 20.0 to 180.0 |
| `tolerance` | Simplify tolerance (px) | float | `0.4` | 0.05 to 3.0 |
| `min_area` | Drop specks smaller than (px²) | float | `4.0` | 0.0 to 1000.0 |

### Round corners (`round_corners`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `max_angle` | Corners sharper than (deg) | float | `60.0` | 10.0 to 170.0 |
| `radius` | Arc radius (px) | float | `3.0` | 0.5 to 50.0 |
| `round_concave` | Also round empty spikes poking into the paint | bool | `false` | true \| false |

### Remove islands (`remove_islands`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `delete_specks` | Delete small paint specks | bool | `true` | true \| false |
| `fill_holes` | Fill small holes | bool | `true` | true \| false |
| `max_area` | Smaller than (px²) | float | `50.0` | 1.0 to 100000.0 |
| `keep_largest` | Keep only the largest shapes | int | `0.0` | 0.0 to 1000.0 |

### Rasterise shapes (`shapes_to_mask`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `resolution` | Resolution (px) | int | `1000.0` | 20.0 to 4000.0 |

### Stencil (`stencil`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `width_mm` | Plate width (mm) | float | `200.0` | 20.0 to 2000.0 |
| `thickness_mm` | Thickness (mm) | float | `5.0` | 0.2 to 100.0 |
| `margin_mm` | Margin (mm) | float | `8.0` | 0.0 to 500.0 |
| `flip` | Print upside down | bool | `false` | true \| false |
| `bridge_mm` | Bridge width (mm) | float | `1.6` | 0.6 to 50.0 |
| `bridges_per_island` | Bridges per island | int | `1.0` | 1.0 to 5.0 |
| `raised_bridges` | Raised bridges | bool | `true` | true \| false |
| `z_bridging` | Z bridging | choice | `"easy"` | easy \| steps |
| `layer_height_mm` | Layer height (mm) | float | `0.2` | 0.05 to 5.0 |
| `extra_top_layers` | Extra top layers | int | `1.0` | 0.0 to 50.0 |
| `max_overhang_deg` | Overhang check angle (deg) | float | `0.0` | 0.0 to 89.0 |
| `min_island_mm2` | Delete islands under (mm²) | float | `0.0` | 0.0 to 100000.0 |
| `min_contact_mm` | Minimum surface contact (mm) | float | `0.0` | 0.0 to 3.0 |
| `min_first_layer_width_mm` | Minimum first-layer width (mm) | float | `0.0` | 0.0 to 10.0 |
| `first_layer_taper_mm` | Taper height (mm) | float | `1.0` | 0.0 to 50.0 |

### Vector stencil (`vector_stencil`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `detail` | Mesh detail | float | `5.0` | 0.5 to 20.0 |
| `width_mm` | Plate width (mm) | float | `200.0` | 20.0 to 2000.0 |
| `thickness_mm` | Thickness (mm) | float | `5.0` | 0.2 to 100.0 |
| `margin_mm` | Margin (mm) | float | `8.0` | 0.0 to 500.0 |
| `flip` | Print upside down | bool | `false` | true \| false |
| `bridge_mm` | Bridge width (mm) | float | `1.6` | 0.6 to 50.0 |
| `bridges_per_island` | Bridges per island | int | `1.0` | 1.0 to 5.0 |
| `raised_bridges` | Raised bridges | bool | `true` | true \| false |
| `z_bridging` | Z bridging | choice | `"easy"` | easy \| steps |
| `layer_height_mm` | Layer height (mm) | float | `0.2` | 0.05 to 5.0 |
| `extra_top_layers` | Extra top layers | int | `1.0` | 0.0 to 50.0 |
| `max_overhang_deg` | Overhang check angle (deg) | float | `0.0` | 0.0 to 89.0 |
| `min_island_mm2` | Delete islands under (mm²) | float | `0.0` | 0.0 to 100000.0 |
| `min_contact_mm` | Minimum surface contact (mm) | float | `0.0` | 0.0 to 3.0 |
| `min_first_layer_width_mm` | Minimum first-layer width (mm) | float | `0.0` | 0.0 to 10.0 |
| `first_layer_taper_mm` | Taper height (mm) | float | `1.0` | 0.0 to 50.0 |
| `tile_method` | Tile method | choice | `"per_tile"` | per_tile \| cut_whole |

### Tile layout (`tile_layout`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `width_mm` | Plate width (mm) | float | `100.0` | 20.0 to 2000.0 |
| `margin_mm` | Margin (mm) | float | `8.0` | 0.0 to 500.0 |
| `max_x` | Largest tile width (mm) | float | `150.0` | 0.0 to 2000.0 |
| `max_y` | Largest tile height (mm) | float | `150.0` | 0.0 to 2000.0 |
| `seam_shift` | Seam may move up to (mm) | float | `8.0` | 0.0 to 50.0 |
| `connector` | Connector shape | choice | `"hex"` | hex \| round |
| `connector_d` | Connector diameter (mm) | choice | `"3"` | 2.5 \| 3 \| 4 \| 5 \| 6 |
| `depth` | Connector depth into each tile (mm) | float | `10.0` | 4.0 to 20.0 |
| `clearance` | Connector hole clearance (mm, diametral) | float | `0.2` | 0.0 to 0.6 |
| `spacing` | Pockets about every (mm) | float | `30.0` | 10.0 to 200.0 |
| `segments` | Round segments | int | `48.0` | 24.0 to 128.0 |
| `edits` | Layout edits | text | `""` | text |

### Remesh (`remesh`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `layer_from` | First layer | int | `1.0` | 1.0 to 1000.0 |
| `layer_to` | Last layer | int | `0.0` | 0.0 to 1000.0 |
| `tolerance_mm` | Largest shift of an outline (mm) | float | `0.1` | 0.01 to 1.0 |
| `smoothing` | Smoothing | float | `0.5` | 0.0 to 1.0 |
| `keep_material` | Only round towards material | bool | `true` | true \| false |
| `max_overhang_deg` | Largest overhang (deg) | float | `45.0` | 10.0 to 90.0 |
| `decimate` | Decimate flat faces and straight edges | bool | `true` | true \| false |
| `decimate_mm` | Largest surface shift (mm) | float | `0.02` | 0.005 to 0.5 |

### Tiles & mounting (old) (`export`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `max_x` | Largest tile width (mm) | float | `0.0` | 0.0 to 2000.0 |
| `max_y` | Largest tile height (mm) | float | `0.0` | 0.0 to 2000.0 |
| `seam_gap` | Seam clearance (mm, total) | float | `0.0` | 0.0 to 1.0 |
| `connector` | Connector shape | choice | `"hex"` | hex \| round |
| `connector_d` | Connector diameter (mm) | choice | `"3"` | 2.5 \| 3 \| 4 \| 5 \| 6 |
| `depth` | Connector depth into each tile (mm) | float | `10.0` | 4.0 to 20.0 |
| `clearance` | Connector hole clearance (mm, diametral) | float | `0.2` | 0.0 to 0.6 |
| `segments` | Round segments | int | `48.0` | 24.0 to 128.0 |
| `pad_count` | Raised pads | int | `0.0` | 0.0 to 4.0 |
| `pad_thread` | Pad thread | choice | `"M8"` | M3 \| M4 \| M5 \| M6 \| M8 \| M10 |
| `pad_height` | Pad height (mm) | float | `6.0` | 1.0 to 30.0 |
| `thread_clearance` | Pad thread clearance (mm, radial) | float | `0.2` | 0.0 to 0.6 |
| `mount_screw` | Mounting screw | choice | `"none"` | none \| M3 \| M4 \| M5 \| M6 \| M8 \| M10 |
| `mount_clearance` | Mounting hole clearance (mm, diametral) | float | `0.3` | 0.0 to 1.0 |
| `mount_threaded` | Cut a real thread | bool | `true` | true \| false |
| `mount_offset` | Distance from a seam to the hole (mm) | float | `10.0` | 4.0 to 60.0 |
| `mount_extend` | Extender plate: distance between the two rows of holes (mm) | float | `20.0` | 8.0 to 100.0 |

### Split volume (`tiles`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `max_x` | Largest tile width (mm) | float | `200.0` | 20.0 to 2000.0 |
| `max_y` | Largest tile height (mm) | float | `200.0` | 20.0 to 2000.0 |
| `seam_shift` | Seam may move up to (mm) | float | `5.0` | 0.0 to 30.0 |
| `seam_gap` | Seam clearance (mm, total) | float | `0.0` | 0.0 to 1.0 |
| `reinforce` | Add material where no connector fits | bool | `true` | true \| false |
| `connector` | Connector shape | choice | `"hex"` | hex \| round |
| `connector_d` | Connector diameter (mm) | choice | `"3"` | 2.5 \| 3 \| 4 \| 5 \| 6 |
| `depth` | Connector depth into each tile (mm) | float | `10.0` | 4.0 to 20.0 |
| `clearance` | Connector hole clearance (mm, diametral) | float | `0.2` | 0.0 to 0.6 |
| `segments` | Round segments | int | `48.0` | 24.0 to 128.0 |
| `bridge_islands` | Bridge islands a cut creates | bool | `true` | true \| false |
| `bridge_mm` | Bridge width (mm) | float | `1.6` | 0.4 to 6.0 |
| `bridges_per_island` | Bridges per island | int | `1.0` | 1.0 to 4.0 |
| `raised_bridges` | Raised bridges | bool | `true` | true \| false |
| `z_bridging` | Z bridging | choice | `"easy"` | easy \| steps |

### Add pads & mounting holes (`pads`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `pad_thread` | Pad thread | choice | `"M4"` | none \| M3 \| M4 \| M5 \| M6 \| M8 \| M10 |
| `pad_height` | Pad & mounting height above the plate (mm) | float | `4.0` | 1.0 to 20.0 |
| `thread_clearance` | Pad thread clearance (mm, radial) | float | `0.2` | 0.0 to 0.6 |
| `long_edge` | Add pads along edges longer than (mm) | float | `300.0` | 0.0 to 2000.0 |
| `mount_screw` | Mounting hole screw | choice | `"M4"` | none \| M3 \| M4 \| M5 \| M6 \| M8 \| M10 |
| `mount_threaded` | Threaded mounting holes | bool | `false` | true \| false |
| `mount_clearance` | Mounting hole clearance (mm, diametral) | float | `0.4` | 0.0 to 1.5 |
| `hole_spacing` | Hole every (mm) | float | `120.0` | 0.0 to 1000.0 |
| `joiner_offset` | Joiner hole distance from a seam (mm) | float | `12.0` | 0.0 to 60.0 |
| `extend` | Extender plate: distance between the two rows of holes (mm) | float | `25.0` | 10.0 to 120.0 |
| `segments` | Round segments | int | `48.0` | 24.0 to 128.0 |

### Expand margin (`ring`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `width` | Border width (mm) | float | `40.0` | 20.0 to 400.0 |
| `rings` | Number of rings | int | `1.0` | 1.0 to 5.0 |
| `slab_length` | Longest slab (mm) | float | `200.0` | 40.0 to 1000.0 |
| `gap` | Gap between parts (mm) | float | `0.0` | 0.0 to 1.0 |
| `joiner_thickness` | Joiner plate thickness (mm) | float | `4.0` | 2.0 to 12.0 |
| `hardware` | Add a bolt and a nut for the joiner plates | bool | `true` | true \| false |
| `screw_length` | Bolt length (mm) | float | `0.0` | 0.0 to 120.0 |
| `nut_clearance` | Nut thread clearance (mm, radial) | float | `0.2` | 0.0 to 0.6 |
| `tip_taper` | Bolt tip taper (mm, radial) | float | `1.0` | 0.0 to 2.0 |
| `tip_pitches` | Bolt tip taper length (thread pitches) | float | `1.0` | 0.5 to 4.0 |
| `number_parts` | Number the slabs | bool | `true` | true \| false |
| `number_size` | Number size (mm) | float | `10.0` | 4.0 to 30.0 |
| `pocket_depth` | Pocket depth (mm) | float | `0.0` | 0.0 to 3.0 |
| `wall_holes` | Mounting holes along the outer edge | bool | `false` | true \| false |
| `hole_spacing` | Hole every (mm) | float | `120.0` | 20.0 to 1000.0 |
| `segments` | Round segments | int | `48.0` | 24.0 to 128.0 |

### Colour set (`colour_set`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `holes` | Registration holes | int | `3.0` | 0.0 to 4.0 |
| `hole_d` | Hole diameter (mm) | float | `4.0` | 2.0 to 12.0 |
| `inset` | Hole distance from the edge (mm) | float | `0.0` | 0.0 to 100.0 |
| `pins` | Add alignment pins to the set | bool | `true` | true \| false |
| `pin_clearance` | Pin clearance (mm, diametral) | float | `0.2` | 0.0 to 1.0 |
| `pin_length` | Pin length (mm) | float | `12.0` | 4.0 to 60.0 |

### Make screws, nuts & plates (`hardware`)

| Parameter | Label | Type | Default | Range or choices |
|---|---|---|---|---|
| `extend` | Extender plate: distance between the two rows of holes (mm) | float | `25.0` | 10.0 to 120.0 |
| `joiner_thickness` | Joiner plate thickness (mm) | float | `4.0` | 2.0 to 10.0 |
| `plate` | Make the hand-sized plates | bool | `true` | true \| false |
| `plate_distance` | Distance between hole centres (mm) | float | `30.0` | 8.0 to 300.0 |
| `plate_height` | Plate thickness (mm) | float | `4.0` | 1.0 to 20.0 |
| `plate_padding` | Padding round the holes (mm) | float | `3.0` | 1.0 to 20.0 |
| `plate_threaded` | Threaded holes | bool | `false` | true \| false |
| `plate_clearance` | Plate hole clearance (mm, diametral) | float | `0.4` | 0.0 to 1.5 |
| `screw` | Screw size | choice | `"M4"` | M3 \| M4 \| M5 \| M6 \| M8 \| M10 |
| `length` | Bolt length (mm) | float | `20.0` | 4.0 to 120.0 |
| `nut_clearance` | Nut thread clearance (mm, radial) | float | `0.2` | 0.0 to 0.6 |
| `tip_taper` | Bolt tip taper (mm, radial) | float | `1.0` | 0.0 to 2.0 |
| `tip_pitches` | Bolt tip taper length (thread pitches) | float | `1.0` | 0.5 to 4.0 |
| `fit_test` | Connector fit-test strip | bool | `true` | true \| false |
| `connector` | Connector shape | choice | `"hex"` | hex \| round |
| `connector_d` | Connector diameter (mm) | choice | `"3"` | 2.5 \| 3 \| 4 \| 5 \| 6 |
| `segments` | Round segments | int | `48.0` | 24.0 to 128.0 |

