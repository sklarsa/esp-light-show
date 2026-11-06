#!/bin/sh
# Renders the step pictures for ASSEMBLY.md (OpenSCAD 2021+).
cd "$(dirname "$0")"
for s in 1 2 3 4 5 6; do
  openscad -D 'part="none"' -D "step=$s" --projection=p --imgsize=1000,800 --colorscheme=Tomorrow \
    --camera=0,0,0,50,0,30,0 --viewall --autocenter -o "step$s.png" steps.scad &
done
wait
