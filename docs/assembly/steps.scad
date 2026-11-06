// Pictures for ASSEMBLY.md. Each step shows what's already in place, with the new part lifted out
// along the way it goes in. Regenerate: see docs/assembly/render.sh
include <../../enclosure.scad>
step = 1;
lift = 30;   // how far the new part floats above its place
module housing() color("royalblue") render() body();
module arrow(from, len = 18) color("red") translate(from) rotate([180, 0, 0]) {
  cylinder(d = 1.6, h = len - 4); translate([0, 0, len - 4]) cylinder(d1 = 4.5, d2 = 0, h = 4);
}
module up(d = lift) translate([0, 0, d]) children();
module c6_set() { c6_board(); led_ribbon(); pir_ribbon(); }
if (step == 1) c6_set();
if (step == 2) { housing(); up() c6_set(); arrow([c6x(0), c6_cy, lift + 6]); }
if (step == 3) { housing(); c6_set(); up() pir_board(); arrow([0, pir_y, lift + 12]); }
if (step == 4) { housing(); c6_set(); pir_board(); up() led_board(); arrow([20, led_oy, lift + 26]); }
if (step == 5) { housing(); c6_set(); pir_board(); led_board();
                 up(14) color("white", 0.9) translate([0, 0, z_rim]) linear_extrude(0.2) offset(delta = 2.5) house(); }
if (step == 6) { housing(); boards(); up(24) color("cornflowerblue") render() frame(); arrow([40, 40, z_rim + 30], 14); }
