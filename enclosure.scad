// esp-light-show enclosure: a little house. LED board in the front, a paper diffuser over it, the
// PIR dome looking out of the plinth, C6 on the back wall with USB out the right-hand side and the
// boot button pressed through the back. Nothing needs screws or glue. Print in PETG, no supports.
//
// Board dimensions come from Apollo's STEP models; PIR sensor and ribbons were measured by hand.
//
//   openscad -D 'part="body"'  -o body.stl  enclosure.scad   (back wall on the bed)
//   openscad -D 'part="frame"' -o frame.stl enclosure.scad   (front face on the bed)
//
// Print the test_* parts first. They're cut from the same geometry, so tune the knobs and reprint:
//   test_c6      back wall: C6 clips, boot tab, USB tunnel
//   test_led     a ring of the LED pocket: board fit and side clips
//   test_pir     plinth: PIR rails and rear stops
//   test_dome    frame's dome hole, to hold against test_pir
//   test_snap_b, test_snap_f   one side of the frame's snap, in a mating pair
//
//   check        boards intersected with the enclosure: the render is empty when nothing collides
part = "assembly";
$fn = 32;

// ---- Knobs. Hardware is never the datasheet. ----
tol = 0.3;        // gap around a board
snap = 0.6;       // how far the frame's snap ridge sticks out; lower if it's too hard to close
c6_lip = 0.5;     // how far the C6 hooks overlap the board; more grips harder but strains the hook
led_lip = 0.9;    // the same for the LED board's side clips
boot_gap = 0.4;   // air between the boot tab and the switch; lower if a press doesn't click
dome_hole = 10;   // the dome's base measured 9.5
dome_y = 11;      // dome centre height; move it if the dome doesn't line up
wall = 2.4;
frame_t = 2;

// ---- Layout. x across, y up, z towards the front; z = 0 is the back face on the bed. ----
led_t = 1.6;
base_h = 27;                  // LED board's bottom edge above the desk
led_oy = base_h + 37.2;       // board's model origin in this frame
led_gap = 7;                  // LED face to paper
z_rim = 33;                   // the body's front edge, where the paper sits
z_led_back = z_rim - led_gap - led_t;
c6_cy = base_h + 15;
c6_dx = -2.68;                // lines the C6's ribbon ports up with the LED board's
c6_top = wall + 6.6;          // the C6 faces the back wall; its tallest part is the 6 mm battery socket
c6_bot = c6_top + 1.59;
// The PIR lies connector-down, so its ribbon reaches the C6 without a twist: the cable's blue
// stiffener is on the same face at both ends. That puts the dome below the white board.
pir_y = dome_y + 7.6;         // the white board's connector face, now its underside
pir_end = z_rim + 8.9 - 32.7; // its ribbon end: 32.7 mm back from the dome tip
half = 32.37 + tol + wall;    // outer half width

// Board outline, from Apollo's STEP model.
house_pts = [[-32.37,-33.91],[-32.07,-34.7],[-31.63,-35.43],[-31.07,-36.06],[-30.4,-36.57],[-29.64,-36.96],[-28.83,-37.2],
  [28.83,-37.2],[29.65,-36.97],[30.41,-36.59],[31.09,-36.08],[31.65,-35.44],[32.09,-34.71],[32.37,-33.91],
  [32.37,-8.51],[32.29,-7.4],[32.07,-6.3],[31.73,-5.24],[31.26,-4.23],[30.66,-3.29],[29.96,-2.42],
  [2.53,25.01],[1.75,25.45],[0.89,25.72],[0,25.81],[-0.89,25.72],[-1.75,25.45],[-2.53,25.01],
  [-29.58,-2.04],[-30.27,-2.88],[-30.88,-3.77],[-31.4,-4.73],[-31.82,-5.72],[-32.15,-6.76],[-32.37,-7.82]];

module house() { translate([0, led_oy]) polygon(house_pts); }

// Everything from the desk up to where the roof starts.
module plinth(grow = 0) {
  translate([-half - grow, -grow]) square([2 * half + 2 * grow, led_oy - 8 + grow]);
}
module outer() { union() { plinth(); offset(r = tol + wall) house(); } }

// C6 parts: x, y and top are in the model's frame, where +z is the top of the board.
function c6x(x) = x + c6_dx;
function c6y(y) = c6_cy - y;           // turned over to face the back wall
function c6z(z) = c6_top + 1.59 - z;

// A hook on a post, one solid profile rooted in the back wall. The board rests on separate pads,
// so the post is thin all the way down and flexes over its whole length.
module post_hook(x, side) {  // side: +1 clips the top edge, -1 the bottom edge
  w = 2.5; t = 1.6;
  e = 9.26 + tol / 2;
  zb = c6_bot + 0.15;  // the hook's flat underside; its chamfer lets the board push past
  root = wall - 0.5;
  translate([x - w / 2, c6_cy, 0]) scale([1, side, 1]) rotate([90, 0, 90]) linear_extrude(w)
    polygon([[e + t, root], [e + t, zb + 1.1], [e, zb + 1.1], [e - c6_lip, zb], [e, zb], [e, root]]);
}

boot = [c6x(13.72), c6y(-7.11)];
reset = [c6x(13.72), c6y(7.11)];
tab_w = 5;

module back_features() {
  // C6 hooks, two on each long edge
  for (x = [8, -12], side = [-1, 1]) post_hook(c6x(x), side);
  // pads the board rests on, in gaps between its parts: beside the RGB LED and beside BOOT
  for (p = [[10.35, 7.6, 1.8], [9, -7.6, 2.4]])
    translate([c6x(p[0]), c6y(p[1]), wall - 0.5]) cylinder(d = p[2], h = c6_top - wall + 0.5);
  // stop at the far end, so pushing the USB plug in can't slide BOOT off the plunger
  translate([c6x(-18.78) - 0.3 - 2, c6_cy + 2, wall - 0.5]) cube([2, 5, c6_bot - wall + 0.5]);
  // and a third rest on the RF can, away from the antenna
  translate([c6x(-12) - 2, c6y(-3) - 2, wall - 0.5]) cube([4, 4, c6z(4.09) - wall + 0.4]);
  // boot tab's plunger, ending just short of the switch
  translate([boot.x, boot.y, 0.8]) cylinder(d = 3, h = c6z(3.54) - 0.8 - boot_gap); // rooted in the thinned tongue
  // USB tunnel's walls, so the plug finds the socket blind
  difference() {
    translate([c6x(17.01) + 0.7, c6_cy - 8.4, wall - 0.5]) cube([half - wall - c6x(17.01) - 0.2, 16.8, 13.1]);
    usb_cut();
  }
}

module usb_cut() {
  translate([c6x(17.01) + 0.2, c6_cy - 7, c6z(2.77) - 5]) cube([half, 14, 10]);
}

module pir_cradle() {
  // Walls either side of the white board, with a ledge under its edges, lips over them and stops
  // behind it. Its connector (11.4 mm) and header hang down between the ledges.
  gx = 6.72 + 0.2;
  for (s = [-1, 1]) scale([s, 1, 1]) {
    translate([gx - 0.8, wall - 0.1, wall]) cube([0.8 + 1.4, pir_y - wall + 0.1, 25.6]);    // ledge
    translate([gx, wall, wall]) cube([1.4, pir_y + 1.6 + 1.2 - wall, 25.6]);                // wall
    translate([gx - 0.9, pir_y + 1.6 + 0.15, pir_end + 4]) cube([2.3, 0.9, 14]);            // lip
    translate([gx - 0.8, pir_y - 2, pir_end - 2]) cube([2.2, 2 + 1.6, 2 - 0.2]);            // stop
  }
}

module led_clips(cut) {  // cut: true for the pocket in the wall, false for the finger in it
  y = led_oy - 14;       // between the LEDs at -21 and -6
  w = 6;
  z0 = z_led_back - 10;
  zf = z_led_back + led_t + 0.15; // the hook's flat underside, just in front of the board
  edge = 32.37 + tol;
  for (s = [-1, 1]) scale([s, 1, 1]) translate([0, y - w / 2, 0])
    if (cut) translate([edge - 1.5, -0.6, z0]) cube([1.5 + 1 + 0.7, w + 1.2, z_rim]);
    else translate([0, w, 0]) rotate([90, 0, 0]) linear_extrude(w)
      polygon([[edge + 1, z0 - 0.5], [edge + 1, zf + 1.1], [edge, zf + 1.1], [edge - led_lip, zf], [edge, zf], [edge, z0 - 0.5]]);
}

module ridge(grow = 0) {  // snap ridge along both straight sides, on the body's outside
  z = z_rim - 3.5;
  // clear of the LED clips' slots at y 47-53; grown at the ends too, so the frame needn't line up exactly
  for (s = [-1, 1]) scale([s, 1, 1]) translate([half, 26 - grow - 0.3, z]) rotate([-90, 0, 0]) linear_extrude(20 + 2 * grow + 0.6)
    polygon([[-0.01, 1 + grow], [-0.01, -1 - grow], [snap + grow, -0.2 - grow], [snap + grow, 0.2 + grow]]);
}

module body() {
  difference() {
    union() {
      difference() {
        linear_extrude(z_rim) outer();
        // behind the LED board: leaves a 1.5 mm ledge for it to sit on
        translate([0, 0, wall]) linear_extrude(z_led_back - wall) union() {
          offset(delta = -1.5 + tol) house();
          translate([-half + wall + 1.2, wall]) square([2 * (half - wall - 1.2), led_oy - 8 - wall]);
        }
        // the LED board's pocket, and the air in front of it
        translate([0, 0, z_led_back]) linear_extrude(z_rim) offset(delta = tol) house();
        // the plinth, under the bar the board stands on
        translate([-half + wall, wall, z_led_back - 0.01]) cube([2 * (half - wall), 23.2 - wall, z_rim]);
        // the boot tab: a tongue, attached at its top end
        translate([boot.x, boot.y, -1]) linear_extrude(wall + 2) difference() {
          translate([-tab_w / 2 - 1, -3]) square([tab_w + 2, 19]);
          translate([-tab_w / 2, -2]) square([tab_w, 19]);
        }
        translate([boot.x - tab_w / 2, boot.y - 2, wall - 0.8]) cube([tab_w, 18, 1]); // thin it to flex
        // room for the PIR ribbon's fold: the back wall is 1.2 mm here, clear of the hooks' roots
        translate([c6x(0.89) - 6, wall, 1.2]) cube([11.4 + 0.6, c6_cy - 9.26 - 3 - wall, 2]);
        // paperclip hole for reset
        translate([reset.x, reset.y, -1]) cylinder(d = 2.5, h = wall + 2);
      }
      // posts carrying the bar over the ribbon's path
      for (x = [-24, -11, 8, 24]) translate([x - 1, 23.2, wall - 0.1]) cube([2, 3.8, z_led_back - wall + 0.2]);
      back_features();
      pir_cradle();
      ridge();
    }
    usb_cut();
    led_clips(true);
    translate([-5, -1, z_rim - 1.5]) cube([10, 1 + 0.6, 2]); // pry notch for the frame
  }
  led_clips(false);
}

module frame() { // modelled in place: its back sits on the body's rim
  clear = 0.2;
  skirt = 1.6;
  depth = 6;
  difference() {
    union() {
      translate([0, 0, z_rim + 0.1]) linear_extrude(frame_t) offset(r = clear + skirt) outer();
      translate([0, 0, z_rim - depth]) linear_extrude(depth + 0.2) difference() {
        offset(r = clear + skirt) outer();
        offset(r = clear) outer();
      }
    }
    translate([0, 0, z_rim - 1]) linear_extrude(frame_t + 3) offset(delta = -0.3) house();
    translate([0, dome_y, z_rim - 1]) cylinder(d = dome_hole, h = frame_t + 3);
    ridge(clear);
    translate([-60, -10, 0]) cube([120, 10, 60]); // flat on the desk, with the body
  }
}

// Bounding solids of the boards, from the STEP models.
module boards() {
  color("white") translate([0, 0, z_led_back]) linear_extrude(led_t) house();
  color("gold") {
    // LED board: buzzer and ribbon connector on the back, LEDs on the front
    translate([17.29, led_oy - 14.91, z_led_back - 4.02]) cube([8.7, 8.7, 4.02]);
    translate([-7.49, led_oy - 3.61, z_led_back - 2.01]) cube([11.4, 6.1, 2.01]);
  }
  for (p = [[-28.2, -33.1], [28.3, -33.1], [14.3, 9.3], [-14.3, 9.1], [-28.7, -6.7], [28.4, -5.8], [29.6, -21.2], [-29.6, -20.8], [0, -34.4], [0, 21.8]])
    color("cyan") translate([p[0] - 1.4, led_oy + p[1] - 1.4, z_led_back + led_t]) cube([2.8, 2.8, 0.84]);
  // C6: board, RF module, ribbon connectors, USB, battery socket, switches
  color("seagreen") translate([c6x(-18.78), c6_cy - 9.26, c6_top]) cube([35.79, 18.52, 1.59]);
  for (b = [[-18.54, -7.39, 13.2, 16.6, 1.59, 4.09], [-4.81, 3.23, 11.4, 6.1, 1.59, 3.59], [-4.81, -9.33, 11.4, 6.1, 1.59, 3.59],
            [9.22, -4.47, 7.6, 8.94, 0.69, 4.85], [-0.97, -2.39, 6, 4.6, -1.91, 7.59],
            [11.32, 5.66, 4.8, 2.9, 1.59, 3.54], [11.32, -8.56, 4.8, 2.9, 1.59, 3.54]])
    color("silver") translate([c6x(b[0]), c6y(b[1] + b[3]), c6z(b[5])]) cube([b[2], b[3], b[5] - b[4]]);
  // PIR, connector down: white board, connector and 3-way header under it, sensor board, dome
  color("white") translate([-6.72, pir_y, pir_end]) cube([13.16, 1.6, 18.63]);
  color("gold") translate([-5.7, pir_y - 2, pir_end + 0.42]) cube([11.4, 2, 6.1]);
  color("dimgray") translate([-4, pir_y - 2.5, pir_end + 8.39]) cube([8, 5.7, 10.15]);
  translate([0, dome_y, 0]) {
    color("black") translate([0, 0, z_rim - 2.6]) cylinder(d = 16, h = 2.6);
    color("ivory") translate([0, 0, z_rim - 0.01]) cylinder(d = 9.5, h = 8.9);
  }
}

module cut(from, to) { intersection() { children(); translate(from) cube(to - from); } }
module flip() { translate([0, 0, z_rim + 0.1 + frame_t]) mirror([0, 0, 1]) children(); } // for printing the frame

if (part == "body") body();
if (part == "frame") flip() frame();
if (part == "assembly") { color("royalblue") body(); boards(); color("royalblue", 0.35) frame(); }
if (part == "open") { color("royalblue") body(); boards(); }
if (part == "check") { intersection() { body(); boards(); } intersection() { frame(); boards(); } }
if (part == "test_c6") cut([-22, c6_cy - 13, 0], [half + 1, c6_cy + 27, 15.6]) body(); // includes the USB tunnel roof
if (part == "test_led") translate([0, 0, -(z_led_back - 11)]) cut([-40, -1, z_led_back - 11], [40, 95, z_led_back + led_t + 3]) body();
if (part == "test_pir") translate([0, 0, -pir_end + 5]) cut([-12, -1, pir_end - 5], [12, 23, z_rim]) body(); // stops under the bar
if (part == "test_dome") flip() cut([-14, -2.5, z_rim - 6.5], [14, 28, z_rim + 3]) frame();
if (part == "test_snap_b") translate([0, 0, -(z_rim - 12)]) cut([half - 8, 22, z_rim - 12], [half + 2, 47, z_rim]) body();
if (part == "test_snap_f") flip() cut([half - 8, 22, z_rim - 7], [half + 4, 47, z_rim + 3]) frame();
if (part == "front") { color("royalblue") body(); boards(); color("royalblue") frame(); }
if (part == "section") { intersection() { union() { color("royalblue") body(); boards(); color("royalblue") frame(); } translate([-60, -5, -5]) cube([60 - 1.8, 110, 60]); } } // through the ribbon ports
