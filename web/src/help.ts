/** A short "why" for the settings people most often wonder about, keyed by the engine's parameter name. */
export const PARAM_HELP: Record<string, string> = {
  width_mm: 'Overall width of the plate. The height follows the picture. Bigger plates print in tiles.',
  thickness_mm: 'How thick the stencil is. Thicker stands up better and keeps paint from bleeding under the edge, but costs print time.',
  margin_mm: 'Solid frame around the artwork. It stiffens the plate and gives room for tape, magnets or registration holes. 0 = no frame.',
  flip: 'Mirror the plate so it prints with the flat face up. Turn it on when the first layer must be the artwork side.',
  bridge_mm: 'Width of the strips that hold floating pieces (islands) to the frame. Wider is stronger but hides more of the picture. Keep it above your nozzle width.',
  bridges_per_island: 'More bridges hold an island more firmly, but each one leaves a gap in the painted shape.',
  raised_bridges: 'Lift the bridges off the first layer so the paint flows under them and the artwork stays whole.',
  z_bridging: 'Steps builds a gentle staircase under each bridge so it never overhangs; easy uses one step and prints faster.',
  layer_height_mm: 'Set this to your slicer layer height. Raised bridges are built in whole layers.',
  extra_top_layers: 'Extra solid layers above the bridges, so the top face is flat and strong.',
  max_overhang_deg: 'Largest overhang the printer can bridge, in degrees from vertical. 90 means no limit.',
  min_island_mm2: 'Floating pieces smaller than this are deleted instead of bridged. 0 keeps all of them.',
  min_contact_mm: 'Material thinner than this is widened with a bridge strip so the nozzle can print it. 0 = off.',
  detail: 'How finely curves are flattened into straight edges. Higher is smoother but makes a much bigger mesh and a slower run.',
  threshold: 'Brightness that splits paint from plate. 256 picks it automatically.',
  resolution: 'Pixels along the longest side. Higher keeps fine detail but is slower; the stencil cannot be finer than a nozzle anyway.',
  smooth: 'Blur before cutting out, to remove jagged pixel edges. Too much rounds off small details.',
  radius: 'How far the effect reaches, in pixels.',
  tolerance: 'How far the traced outline may move to use fewer points. Higher = simpler outline, less accurate.',
  tolerance_mm: 'How far an outline may move to use fewer vertices. Lower keeps the shape, higher gives a lighter mesh.',
  corner_deg: 'Corners sharper than this stay sharp; gentler bends are smoothed into curves. 180 rounds everything.',
  min_area: 'Specks smaller than this are dropped as noise. 0 keeps everything.',
  max_angle: 'Corners sharper than this are rounded. Very pointy tips are fragile to cut and to print.',
  max_area: 'Anything smaller than this is removed.',
  keep_largest: 'Keep only this many of the biggest shapes; handy to drop noise around a logo. 0 keeps all.',
  smoothing: 'Strength of smoothing across layers: 0 changes nothing, 1 is the most aggressive.',
  decimate: 'Collapse vertices on flat faces and straight edges. Creases and the closed mesh are kept; only the shift below is allowed.',
  decimate_mm: 'How far the surface may move when decimating. Higher removes more triangles; 0.02 mm is invisible, 0.1 mm still keeps every crease.',
  keep_material: 'Round only towards solid material, so the stencil is never thinner than the original.',
  max_x: 'Largest tile width. The plate is cut into tiles that fit your printer bed. 0 = no cutting.',
  max_y: 'Largest tile height. 0 = no cutting.',
  connector_d: 'Diameter of the rods that join tiles. Print the fit-test strip first to pick the clearance.',
  seam_shift: 'How far a seam may move off the even grid to cross less artwork. It never makes a tile larger than the largest size, so there is no room to move when the plate divides exactly.',
  spacing: 'Distance between connectors along a seam.',
  connector: 'Shape of the connector rod: hex or round.',
  depth: 'How far each connector rod goes into a tile. Deeper joins are stiffer.',
  clearance: 'Gap between a rod and its hole. Too tight will not fit, too loose wobbles.',
  segments: 'Facets used for round shapes. More is smoother but a heavier mesh.',
  holes: 'Registration holes drilled in the margin of every colour plate. Two sit on a diagonal and three form an L, so a plate cannot be laid on rotated.',
  hole_d: 'Diameter of the registration holes. The margin must be at least this plus 1 mm.',
  inset: 'Distance of the hole centres from the plate edge. 0 puts them in the middle of the margin.',
  pins: 'Also output a small set of pins to push through the holes so the plates stay lined up.',
  pin_clearance: 'How much thinner than the hole the pins are printed.',
  pin_length: 'Length of each pin. It should go through the plate and a little into the wall or backing.',
  colours: 'How many flat colours to keep. Each becomes its own stencil.',
  supersample: 'Renders at a higher scale and averages, for smoother edges.',
  angle: 'Direction of the pattern lines on tones that set none.',
  bridge_islands: 'When a cut leaves a piece floating free, add a bridge so it stays attached. Turn off only if you will join the pieces yourself.',
  crop_left: 'Trim this share of the picture width off the left side.',
  crop_right: 'Trim this share of the picture width off the right side.',
  crop_top: 'Trim this share of the picture height off the top.',
  crop_bottom: 'Trim this share of the picture height off the bottom.',
  delete_specks: 'Remove tiny paint specks left by noise, so they do not become useless little holes in the stencil.',
  extend: 'Distance between the two rows of holes on the four-hole extender plate. It reaches this far past the seam to hold the parts.',
  fill_holes: 'Fill small unpainted spots inside a shape. Each one would otherwise be an island needing bridges.',
  fit_test: 'Also print a short strip with connector holes of growing clearance. Try your rods in it and pick the clearance that fits.',
  flip_h: 'Mirror the picture left to right.',
  flip_v: 'Mirror the picture top to bottom.',
  font: 'Typeface used for the text. Bold faces make sturdier stencil letters.',
  gap: 'Space left between neighbouring parts so they still fit after printing swells the edges. 0 makes them touch. The holes do not move.',
  hardware: 'Also output a bolt and a nut for the joiner plates, sized to fit the joint.',
  hole_spacing: 'Along a long edge, put a mounting hole every this many millimetres. 0 means corner holes only.',
  invert: 'Swap paint and plate: dark becomes open and light becomes solid.',
  iterations: 'How many times the operation is applied. 0 repeats until nothing changes (thinning only).',
  joiner_offset: 'Each seam gets a hole this far on each side of it, so a joiner plate bridges the seam. 0 = no joiner holes.',
  joiner_thickness: 'Thickness of the joiner plates. Thicker is stiffer; keep it at least a few layers.',
  kernel: 'Shape of the brush used to grow or shrink the paint. A disc is the most natural; the others favour certain directions.',
  kind: 'Pattern given to tones that have no pattern of their own.',
  layer_from: 'First layer kept. 1 is the layer on the plate.',
  layer_to: 'Last layer kept. 0 means up to the top one.',
  length: 'Overall length of the bolt, head included. Pick one that passes through both parts and the nut.',
  line_spacing: 'Distance between lines of text, as a multiple of the normal spacing.',
  long_edge: 'Edges longer than this also get pads along them. 0 = pads at the corners only.',
  mount_clearance: 'Extra width of the mounting holes over the screw, so the screw passes freely. Raise it if screws bind.',
  mount_extend: 'Distance between the two rows of holes on the four-hole extender plate made for these parts.',
  mount_offset: 'How far the screw hole sits from the seam. Larger leaves more material around the hole but needs bigger plates.',
  mount_screw: 'Size of the screw that joins the parts. none = no mounting holes.',
  mount_threaded: 'Cut a real thread in the holes so the screw bites into the plastic. Off leaves a plain hole for a bolt and nut.',
  number_parts: 'Engrave a number on each slab so you can put them in order when assembling.',
  number_size: 'Height of the engraved numbers.',
  nut_clearance: 'Gap between bolt and nut threads. Raise it if the nut is tight on your printer, lower it if it is loose. The same value is used by every node that makes bolts and nuts.',
  pad_count: 'Raised pads with a threaded hole, for screwing the finished stencil to a wall or frame. 0 = none.',
  padding: 'Blank border added round the text, in pixels.',
  pad_height: 'How tall the raised pads stand above the plate.',
  pad_thread: 'Thread size of the pads. Pick the screw you have.',
  pattern: 'The pixel pattern looked for: isolated points, ends of lines, corners or junctions.',
  plate: 'Also make joiner plates to the size set here by hand, whatever is connected to the node.',
  plate_clearance: 'Extra width of the plate holes over the screw. Used for plain holes; threaded ones use half of it.',
  plate_distance: 'Distance from the centre of one hole to the centre of the other.',
  plate_height: 'Thickness of the plate.',
  plate_padding: 'Material between the edge of a hole and the edge of the plate, all round.',
  plate_threaded: 'Cut a real thread in the plate holes, so the bolt screws in. Off makes plain holes the bolt passes through.',
  pocket_depth: 'How deep the number pockets are cut. 0 uses the plate layer height.',
  reinforce: 'Add material where no connector fits between tiles, so the joint is not left weak.',
  rings: 'How many rings of slabs to add round the plate. More rings make the border wider.',
  rotate: 'Also match the pattern turned by 90°, 180° and 270°.',
  round_concave: 'Round off thin spikes of empty space that poke into the paint, as well as the paint corners.',
  screw: 'Screw size. The bolt, nut and holes all follow it.',
  screw_length: 'Length of the bolt. 0 chooses one that fits the joint: plates, joiner and nut.',
  size: 'Height of the letters, in pixels.',
  slab_length: 'Longest piece the border is cut into. Shorter slabs fit smaller printers.',
  thread_clearance: 'Gap between the thread of the screw and of the hole. Raise it if screws are tight.',
  tip_pitches: 'How far back from the end the bolt tip narrows, counted in thread pitches. The same value is used by every node that makes bolts and nuts.',
  tip_taper: 'How much narrower the very end of the bolt is, so it finds the hole. 0 = a plain end. The same value is used by every node that makes bolts and nuts.',
  turn: 'Turn the picture clockwise in steps of 90°.',
  wall_holes: 'Also drill mounting holes along the outer edge of the border, to fix the stencil to a wall.',
  width: 'Width of the border added round the plate, edge of plate to outer edge.',
  plate_width_mm: 'Width of the plate the patterns are laid out for. Set it to the same width as the Stencil node so the spacing comes out in real millimetres.',
  spacing_mm: 'Distance between pattern lines or dots on tones that set none. 0 makes them solid.',
  text: 'The words to trace. Enter starts a new line.',
  custom: 'A 3x3 pattern to look for, one row per line: 1 is paint, 0 is empty, x is either.',
  kind_N: 'Pattern for this tone. auto picks one from how light the tone is.',
  spacing_N: 'Distance between lines or dots for this tone. 0 uses the default spacing above.',
  cover_N: 'How much of this tone is paint, in percent. 0 lets the engine choose from the tone.',
  op: 'What to do to the paint. erode shrinks it, dilate grows it, open removes specks, close fills small gaps, thin and skeleton reduce shapes to lines; the others pick out edges, small bright or dark details, or a hand-made pattern (hit_or_miss).',
  tile_method: 'Used with a Tile layout. per_tile bridges every tile on its own. cut_whole bridges the plate once, cuts it into tiles, and re-bridges only the pieces that came loose.',
}

/** Where one node means something different by the same parameter name. */
export const NODE_HELP: Record<string, string> = {
  'pads.hole_spacing': 'Along a long edge of the margin, put a mounting hole every this many millimetres. 0 means corner holes only.',
  'ring.hole_spacing': 'Along the outer edge of the border, put a wall hole every this many millimetres.',
}

/** Help for a parameter: the node's own wording first, then the shared one; per-tone settings drop their number. */
export function helpFor(nodeType: string, name: string): string | undefined {
  return NODE_HELP[`${nodeType}.${name}`] ?? PARAM_HELP[name] ?? PARAM_HELP[name.replace(/_\d+$/, '_N')]
}

/** The unit written in a label's parentheses, shown beside the number box. */
export function unitOf(label: string): string | null {
  for (const m of label.matchAll(/\(([^)]*)\)/g)) {
    const u = /(mm²|mm|px²|px|deg|degrees|%)/.exec(m[1])
    if (u) return u[1] === 'degrees' ? 'deg' : u[1]
  }
  return null
}

/** The label without its unit: the unit shows beside the number box. "(mm)" goes, "(mm, 0 = none)" becomes "(0 = none)". */
export function plainLabel(label: string): string {
  return label
    .replace(/\((mm²|mm|px²|px|deg|degrees)(?:[,;]\s*([^)]*))?\)/g, (_, _u, rest) => (rest ? `(${rest})` : ''))
    .replace(/\s+/g, ' ')
    .trim()
}
