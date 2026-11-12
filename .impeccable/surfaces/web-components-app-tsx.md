---
version: 1
slug: "web-components-app-tsx"
primary_target: "web/components/App.tsx"
related_targets: []
---

Scope: web/ single page (the game screen). Mode: operate. Audience: hackathon judges in a 75 s demo + one-time strangers; must read in 5 s. Job: connect → claim seed → pick a city plot → plant → watch a round settle → verify.

## Direction contract
THESIS: Drizzle is a night-time farm of 12 city plots; rain waters crops and coins pop out like a harvest. Refuses the category default of a crypto dashboard with stat cards, feed column and parameter tables on one screen.
OWN-WORLD: dark night-sky ground over warm soil plots; rain cyan, coin gold, sprout green as data colors; one seed-packet orange used only on pressable controls. Chunky pixel display face for signs and headings, rounded workhorse sans with tabular numerals for numbers. Authored SVG crops in growth stages, soil furrows, cloud and drops; no emoji.
STORY: the visitor sees which plots just got rain, understands planting earns from rain and dry plots wilt, and plants in one obvious step.
FIRST VIEWPORT: slim top bar (wordmark, Replay/Live switch, round pill with countdown, wallet + coin balance). Board: 12 plots in a 4x3 grid (2 cols on mobile), each plot = sign with city, weather above, crop sized by stake, last mm. Right column (bottom sheet on mobile): action card showing only the next step. Rounds and How-it-works live in a side drawer.
FORM: Crop Plots (farming-sim) seed:dbba7fc56041d53b, chosen as impeccable's pick. Raises: action-only accent and plain labels (from warm consumer app); changed plots stay lit until noticed (from gate board).
SIGNATURE: planting = seed arcs from the action card into the plot, soil puff, seedling grows; settlement = rain falls on wet plots with gold +N coins popping, dry plots crack and droop with −0.1%.
