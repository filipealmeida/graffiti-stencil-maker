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
  max_overhang_deg: 'Largest overhang the printer can bridge. 0 uses 45°.',
  min_island_mm2: 'Floating pieces smaller than this are deleted instead of bridged. 0 keeps all of them.',
  min_contact_mm: 'Material thinner than this is widened with a bridge strip so the nozzle can print it. 0 = off.',
  detail: 'How finely curves are flattened into straight edges. Higher is smoother but makes a much bigger mesh and a slower run.',
  threshold: 'Brightness that splits paint from plate. 256 picks it automatically.',
  resolution: 'Pixels along the longest side. Higher keeps fine detail but is slower; the stencil cannot be finer than a nozzle anyway.',
  smooth: 'Blur before cutting out, to remove jagged pixel edges. Too much rounds off small details.',
  radius: 'How far the effect reaches, in pixels.',
  tolerance: 'How far the traced outline may move to use fewer points. Higher = simpler outline, less accurate.',
  tolerance_mm: 'How far an outline may move to use fewer vertices. Lower keeps the shape, higher gives a lighter mesh.',
  corner_deg: 'Corners sharper than this stay sharp; gentler bends are smoothed into curves.',
  min_area: 'Specks smaller than this are dropped as noise.',
  max_angle: 'Corners sharper than this are rounded. Very pointy tips are fragile to cut and to print.',
  max_area: 'Anything smaller than this is removed.',
  keep_largest: 'Keep only the biggest shapes; handy to drop noise around a logo.',
  smoothing: 'Strength of smoothing across layers: 0 changes nothing, 1 is the most aggressive.',
  decimate: 'Collapse vertices on flat faces and straight edges. Creases and the closed mesh are kept; only the shift below is allowed.',
  decimate_mm: 'How far the surface may move when decimating. Higher removes more triangles; 0.02 mm is invisible, 0.1 mm still keeps every crease.',
  keep_material: 'Round only towards solid material, so the stencil is never thinner than the original.',
  max_x: 'Largest tile width. The plate is cut into tiles that fit your printer bed. 0 = no cutting.',
  max_y: 'Largest tile height. 0 = no cutting.',
  connector_d: 'Diameter of the rods that join tiles. Print the fit-test strip first to pick the clearance.',
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
}

/** The unit written in a label's parentheses, shown beside the number box. */
export function unitOf(label: string): string | null {
  for (const m of label.matchAll(/\(([^)]*)\)/g)) {
    const u = /(mm²|mm|px²|px|deg|degrees|%)/.exec(m[1])
    if (u) return u[1] === 'degrees' ? 'deg' : u[1]
  }
  return null
}
