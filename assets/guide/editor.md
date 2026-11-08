# Start here

The Miniscent editor builds levels out of brushes: boxes and other convex shapes that either carve space out of solid rock or put solid back. Lights, a player start and a goal turn the space into a level, and a script beside it says what happens. Press **F5** at any time to play what you have.

## Your first five minutes

1. File, New gives you an empty level. Everything starts as solid rock.
2. Press **B** for the Box brush. Check that New brush, on the left, is set to the minus button: a new brush carves.
3. Drag a rectangle in the Top view. That carves a room 4 units tall. The 3D view shows it from inside within a moment.
4. Press **P** and click inside the room in the Top view: that is where the player starts.
5. Press **L** and click in the room: a light. The 3D view bakes its light by itself.
6. Press **G** and click somewhere in the room: reaching the goal wins.
7. Press **F5** to play. **Esc** comes back.

## Reading this manual

**F1** opens this manual on the page about the tool in hand. The pages on the left go from the basics to recipes. The cookbook pages are step-by-step recipes; Keys lists every shortcut, and Troubleshooting explains the messages you may see.

The scripting language has a guide of its own, on the Script pane's Guide tab: press **F7**.

# The window

## Areas

- The menu bar is at the top. The status bar at the bottom shows, from the left: the last message, the cursor's position in a 2D view, the level's counts and grid (with **[modified]** when there are unsaved changes), and the state of the 3D picture.
- The tool palette is on the left: the tools, the New brush buttons (carve or solid) and the Grid size.
- The views are in the middle: three 2D views (Top, Front, Side) and the 3D view.
- The properties panel is on the right. It shows what is selected: a brush, its faces, an entity, a voxel volume's tools. Under it, the Lighting log reports bakes, warnings and errors.
- The Script pane opens under the views with **F6**, or in a window of its own.

## Selection

One thing is selected at a time: a brush or an entity. There is no multiple selection. Faces are the exception: the Face tool picks several faces at once.

## The status bar's last field

- **Updating preview...**: the level is being rebuilt after an edit.
- **Baking lighting...**: the picture shows draft lighting while the full bake runs.
- **Lighting up to date**: what you see is what plays.
- **Draft lighting: bake failed**: the bake could not finish; the log says why.
- **Preview not updated**: the last edit could not be built; the message and the log say why.

## Undo

**Ctrl+Z** undoes and **Ctrl+Y** (or **Ctrl+Shift+Z**) redoes. One drag, one slice, one carve, one panel edit or one voxel stroke is one step. Undo history is lost on New, Open and Open showcase level.

# Views and the camera

## The 2D views

| View | Axes |
|---|---|
| Top | X to the right, Y up the screen. Z is height. |
| Front | X and Z, looking along +Y. |
| Side | Y and Z, looking along -X. |

- **Middle drag** pans.
- **Wheel** zooms around the cursor.
- **Left button** does what the tool in hand does.
- Each 2D view keeps the whole level framed until you pan, zoom or click in it. View, Fit views to level frames it again.
- Every position snaps to the grid. There is no way to turn snapping off, so pick a finer grid for finer work.
- The 3D camera shows in the 2D views as a magenta dot with its view cone.

## Grid size

Pick 0.25, 0.5, 1, 2, 4 or 8 units in Grid size on the palette. The grid also sets how far Extrude pushes a face and how small a resized box can get. The player's ship is a ball 1 unit across, so a grid of 1 is right for most rooms and corridors.

## The 3D view

Click it first, so it has the keyboard.

| Input | What it does |
|---|---|
| Right button held | Look around |
| Arrow keys | Look around |
| W, A, S, D | Move forward, left, back, right |
| R, F | Move up, down |
| Q, E | Roll |
| Shift | Move three times as fast |
| Left click | Select (Select tool), pick a face (Face tool) or sculpt (Voxel tool) |

Hold the right button while you fly. Without it, Q and E are also the Select and Enemy tool keys, and pressing them changes the tool.

The keys are by position: on an AZERTY keyboard, ZQSD moves.

View, Reset 3D camera puts the camera at the player start, level and unrolled. It is the only way to undo a roll.

The wheel in the 3D view does not move the camera. It sets the radius of the Voxel tool, and rolls a new result when a voxel generator's result is waiting.

## Layouts

View, Quad layout (the default) shows Top and 3D above Front and Side. View, Classic layout shows the three 2D views in a row with the 3D view below. The dividers between views can be dragged.

View, Switch 3D view between GPU and software chooses the renderer of the 3D view. The game plays the same on both; the GPU picture can look better.

# Brushes and CSG

## The one rule

The world starts as solid rock. Brushes are applied in order, first to last. At any point in space, the **last** brush that contains the point decides: if it is a subtract brush the point is empty, if it is an add brush the point is solid. A point no brush contains stays solid.

- A **subtract** brush (blue in the 2D views) carves empty space. Rooms and corridors are subtract brushes.
- An **add** brush (orange) puts solid back: pillars, ledges, steps, crates.
- An add brush only shows where it lies inside empty space carved before it. An add brush with nothing carved around it changes nothing.

## Order

A new brush goes last, so it wins over everything already there. To change that, select a brush and use Edit, Move later in order or Move earlier in order. The properties panel shows the position as **Brush N of M**.

- A pillar is an add brush **after** the room it stands in. Moved before the room, the room carves it away.
- A doorway is a subtract brush that overlaps two rooms' walls. Its order does not matter as long as nothing solid comes after it.

## Add or subtract

The New brush buttons on the palette set what the next new brush is: minus carves, plus is solid. To change a brush that exists, use Operation in the properties panel or Edit, Flip add/subtract.

## Shapes

- A **box** has resize handles and Min and Max fields.
- A **convex** shape is what a box becomes once its corners are moved off the axes, or what slicing makes. It can be moved, sliced and carved, but not resized by handles or fields.
- A **voxel volume** is sculpted: see Voxels.

## Limits

- Every coordinate stays within -4096 to 4096.
- A brush must have a positive size on all three axes: **Brush must have a positive size**.
- A shape has at most 48 corners for the Move corners tool.

# Brush tools

## Select / move (Q)

- Click a brush or an entity in a 2D view to select it. Entities win over brushes, and among brushes the smallest under the cursor wins. In the 3D view a click selects the brush whose surface is under it; a click on nothing clears the selection.
- Drag the selection to move it by whole grid steps.
- A selected box shows eight handles: four corners and four edge middles. Drag one to resize. A box keeps at least one grid step on every side.
- The properties panel edits the selected brush exactly: Min and Max, Operation and Material.

## Box brush (B)

Drag a rectangle in a 2D view. The new box is 4 units deep along the axis the view looks down: Z from 0 to 4 in the Top view, Y from 0 to 4 in the Front view, X from 0 to 4 in the Side view. Resize it in another view, or type its Min and Max. It takes the New brush operation and the last material picked, and it is selected.

## Slice brush (C)

1. Select a brush, or let the slice pick the one under the cursor.
2. Drag a line across it in a 2D view. The cut runs through the brush's whole depth along the view's axis.
3. Press **K** to choose what to keep: both pieces, the right side or the left side of the line, as drawn. Ticks on the line show the side kept.
4. Press **Enter** to cut.

Keeping both makes a second brush right after the first in the order. **Esc** or another tool drops the line.

## Move corners (V)

Select a brush, then drag the yellow corner handles in a 2D view. A handle grabs every corner it covers in that view, so an edge seen end on moves as one. The shape stays convex: a corner that would end up inside the shape, on another corner or out of range snaps back with **A corner cannot go there**. A moved box becomes a convex shape.

## Carve with brush (Ctrl+Shift+C)

Select a brush and press **Ctrl+Shift+C**. It is subtracted from every brush it overlaps, whatever their operation, and then deleted. Each overlapped brush is split into convex pieces, and the faces the cut opens get default texture mapping. Use it to cut a doorway into a wall made of add brushes, or to notch a shape you built.

## Extrude face out and in (Ctrl+E, Ctrl+Shift+E)

Pick one or more faces with the Face tool (T), then:

- **Ctrl+E** adds a slab one grid step thick on each picked face, with the brush's operation and material. Its outer face becomes the new pick, so pressing again keeps pushing.
- **Ctrl+Shift+E** moves the picked faces one grid step inward.

## Other Edit commands

- **Delete** removes the selection. It is undoable.
- **Flip add/subtract** changes the selected brush's operation.
- **Make mover** turns the selected add brush into a moving part: see Scripts and movers.

# Faces and textures

## Picking faces (T)

The Face tool works in the 3D view.

| Input | What it does |
|---|---|
| Left click | Pick the face under the cursor (and select its brush) |
| Ctrl + left click | Add or remove a face from the picks |
| Left click on nothing | Clear the picks |
| Alt + left click | Copy the face's texture |
| Alt + right click | Paste the copied texture on the face under the cursor |

Only faces you can see in the built level can be picked: faces buried in solid or behind others are not there.

Without picks, the face panel edits every face of the selected brush, whatever the tool.

## The Face / texture panel

- **Materials**: click a thumbnail to put it on the faces. The brush's own Material list (with any other tool) sets every face of the brush.
- **Scale U and V**: world units per texel. Smaller is a finer, more repeated texture. A negative scale mirrors it.
- **Shift U and V**: slides the texture, in texels.
- **Rotation**: in degrees.
- **Tiles U and V** and **Fit**: stretches the texture so it repeats exactly that many times across the face.
- **Left, Right, Top, Bottom, Center**: lines the texture's edge or middle up with the face's.
- **Align face** and **Align world**: lays the texture on the face's own plane, or projects it from the world axes, so neighbouring faces line up.
- **Flip U** and **Flip V** mirror it.
- **Copy** and **Paste** do what Alt + click does.
- **Texture moves with brush**: when on, moving the brush carries the texture with it. When off, the texture stays fixed in the world, so it lines up across brushes.
- The last line gives the texture's size and how many units one tile covers.

Pasting onto a face that shares an edge with the copied one continues the texture across the edge, so walls wrap around corners without a seam.

## Materials

The stock materials are brick, floor, lightpanel, metal and rock. Materials made in the Texture Studio join the list once applied. View 0 to View 7 at the end of the list are not textures: they are windows onto vistas (see Vistas and mountains).

# Entities

Entities are points in the level: where the player starts, lights, enemies, the goal, and the named places a script uses. Pick the tool and click in a 2D view.

## Where they land

A click sets the two coordinates the view shows. The third is fixed: **Z = 1.5** from the Top view, **Y = 0** from the Front view and **X = 0** from the Side view. Place from the Top view in rooms whose floor is at Z = 0, or set Pos X, Y and Z in the Entity panel afterwards. A box drawn in the Top view runs from Z = 0 to 4, so a Top view click lands inside it.

## The kinds

| Kind (key) | What it is |
|---|---|
| Player start (P) | Where the player's ship begins, facing Yaw (the arrow in the Top view). A level has one: placing another replaces it. |
| Light (L) | A lamp, baked into the lighting. See Lighting. |
| Projector (J) | A spot lamp shining one way. See Lighting. |
| Enemy (E) | An enemy. Kind picks one of the level's enemy kinds, counted from 0. A level with no kinds of its own has two: 0 the drone and 1 the hunter. The 3D view shows each enemy as its kind's model, facing +X as it does before it first moves. |
| Goal (G) | Reaching within its Radius wins. |
| Marker (M) | A named point for a script: a spawn point, a hinge. |
| Zone (Z) | A box a script hears about when the player enters or leaves. Half X, Y and Z are half its size. |
| Vista layer (Y) | The eye of a vista, in a sealed room. See Vistas and mountains. |

## Editing values

Type in a field and press **Enter**, or leave it: that commits one undo step. Text that is not a number goes back to what it was.

- A value an entity cannot have is refused with **Entity value out of range**, and nothing changes. Light colours go from 0 to 1, so use the field, not the arrows, which step by 1. Radius and half sizes must be above 0.
- A projector's and a vista layer's values are kept in range for you instead: a pitch past 90 becomes 90.
- An enemy's Kind is kept inside the level's kinds: in a level with the stock two, 5 becomes 1. A level whose enemy names a kind it does not have will not load, so it cannot be saved either.
- Angles are in degrees.

## The world entity

Edit, Level settings sets the fog. It makes a World entity, drawn at the origin as a blue circle. Delete it to go back to the default fog.

## Names and tags

The Script names group gives an enemy, a marker or a zone a name a script can find, and tags it can share. See Scripts and movers.

# Lighting

## Lamps

- A **Light** shines in every direction. **Red**, **Green** and **Blue** are from 0 to 1. **Radius** is where its light ends: it fades smoothly to nothing there. **Intensity** multiplies it.
- A **Projector** shines one way. **Yaw** turns it about the vertical (0 points along +X), **Pitch** tilts it (-90 straight down, the default). **Inner angle** is the full-strength core, **Outer angle** the edge of the soft cone. The 2D views draw its cone when it is selected.

Walls cast soft shadows from both, and light bounces once. Fog does not change the lighting.

## Baking

There is no bake button. After each edit the 3D view shows draft lighting at once (**DRAFT LIGHT**), and once the level has been still for a moment the full bake runs (**BAKING LIGHT**). The showcase level bakes in well under a second; a large level can take many seconds. Keep editing: an edit that changes the shape cancels the bake and starts over.

The bake is kept in a file beside the level, **level.msl.lit**, written when you save with the light up to date. Opening the level again uses it at once. A stale or damaged file is ignored and baked again.

## Moving lights and shadows

Lamps are static. Shots and flashes light their surroundings briefly while playing. Moving things (the ship, enemies, movers) cast shadows from lamps on the GPU renderer only.

## Fog

Edit, Level settings opens the Fog window. Fog starts at **Fog start** units from the eye and is full at **Fog end**, at most 1000. Without a World entity the game uses 4 to 26. Each vista layer has a fog of its own.

## The Lighting log

The box under the properties panel lists bake times, cache use, warnings and errors. It shows the newest lines.

# Scripts and movers

What a level does, from doors to waves of enemies, is in its script: a text file beside the level, **foo.st** beside **foo.msl**. **F6** shows the Script pane and **F7** opens its guide, which teaches the language. This page covers what the editor side needs.

## Names

Select something, type in **Name** and **Tags** in the Script names group, and press **Apply names**.

- Names and tags are lower case letters, digits and _, at most 32 characters. A level's names are all different; tags can be shared, up to 8 on one thing.
- Scripts find movers, zones, markers and enemies by name or tag. A name on a light, a projector, the player start, a goal or a vista layer is kept, but no script can use it.
- The group lists the script lines that use the name. To rename, rename in both places.
- Clearing both fields removes the name.

## Movers

A mover is a part of the level that moves: a door, a lift, a platform. Select an add brush and use Edit, Make mover, or give it a name. Brushes with the same name move together, and a mover can be any shape built from add brushes. A subtract brush cannot move: a moving hole has no meaning. Flipping a mover to subtract takes it out of its mover when the level is built.

A zone named after a mover with **_area** at the end, such as **door_1_area**, moves with it.

## Add entity

Script, Add entity (or the buttons on the Script tab) places a working piece and its script lines in one step: a door, a locked door, a switch door, a lift, a trigger, an enemy wave, a terminal, a health pack or an exit. Click where it goes in the Top view. Doors and lifts fit themselves to the smallest carved room around the click. Their names are the kind and a number, such as **door_1**.

## Samples

Script, Open sample opens a small finished level with its script. Your copy is yours to change; opening the sample again starts it over.

# Voxels

Voxel volumes are sculpted, not built: caves, rubble, roots, rock. A volume is a brush in the order like any other, made of small cells you add to or carve with a round dab.

## Sculpting (X)

The Voxel tool works in the 3D view. Choose what a dab does in the Voxel panel, then:

| Input | What it does |
|---|---|
| Left drag | Sculpt along the stroke |
| Ctrl + left drag | Add instead of Carve, or Carve instead of Add |
| Alt + left drag | Build up: each pass adds to the last, where a plain stroke keeps one even depth |
| Wheel | Radius, from 0.25 to 8 |
| Esc | Cancel the stroke, end a tunnel, or drop a generated result |

There is no need to make a volume first. **Add** where there is none makes an add volume around the dab. **Carve** on a brush wall makes a subtract volume that matches the wall, so nothing changes until the first dab bites. A stroke grows its volume as it goes.

One stroke is one undo step.

## The dabs

| Dab | What it does |
|---|---|
| Add, Carve | Puts on or digs out solid along the stroke |
| Smooth | Rounds what is there off |
| Paint | Changes the material only |
| Flatten | Pulls the surface to the plane where the stroke started |
| Roughen | Adds bumps, from the Seed |
| Tunnel | Click a path: each click carves a round tunnel from the last one, at the radius of that click. Esc ends it |
| Copy | Takes a ball of the volume under the click |
| Stamp | Puts the copy down at the click. The cell size must match |

**Material** is what new solid is made of. **New cells** is the cell size of a volume a stroke creates: 0.125, 0.25 (the default), 0.5 or 1 unit. Smaller is finer but covers less: a volume is at most 256 cells on a side.

## Generators

Choose one in **Generate**: Stalactites, Mushrooms, Coral, Tree, Hive or Mountains. Click a surface. The result is held, not kept: the wheel or the Seed field rolls another, **Commit** keeps it as one undo step, and **Esc** drops it. Saving, playing, picking a dab and most other edits keep it; changing tool drops it. The size follows the radius. Stalactites grow along the surface, so they hang from a ceiling and rise from a floor.

## Fill

Select a volume and press **Fill selected volume**: every cell is replaced from a warped gyroid or cave noise, with **Passage** setting the width of the passages. It replaces sculpting too.

## Working with volumes

- Select a volume with Select to see its box. It can be moved, reordered and deleted, but not resized: strokes grow it.
- Volumes may touch but not overlap. A stroke or a move into another volume is refused.
- Flip add/subtract keeps the sculpted shape.
- Slice, Move corners, Carve with brush and the face panel do not work on volumes.
- Volumes light and cast shadows as brushes do, and collide exactly as drawn.

# Vistas and mountains

A vista is scenery far beyond the walls: stars, a planet, a mountain range. It is a small room of its own, sealed off from the level, seen through faces painted with a View material.

## How it works

- A face painted **View 0** to **View 7** is a window onto that vista slot. It still collides like a wall.
- A **Vista layer** entity is the eye of the slot. It sits in a sealed room: a carved room with solid all around, which the player cannot reach.
- A slot has up to four layers, drawn far (layer 0) to near (layer 3), so mountains can stand in front of the stars.
- **Scale** is how much bigger the vista looks than it is: at 16, one unit in the vista room looks like 16. At 0 it is infinitely far and does not shift as the player moves. **Spin** turns it slowly, in radians per second. **Pan U** and **Pan V** scroll self-lit textures, for clouds.

## New vista

Edit, New vista builds a 64-unit starry room beyond the level, with a planet, and a layer that turns slowly, all in one undo step. Then paint a wall or ceiling with the View material it names in the status bar, such as **View 0**: until a face shows that slot, nothing does.

## New mountains

Edit, New mountains builds a 128-unit room beyond the level with a sculpted mountain range, a lamp and a layer at scale 16. It joins the first vista in the level as its next layer, in front of its stars, or takes a slot of its own when there is none. Its floor lines up with the level's floor so the horizon sits right.

## Rules

The Lighting log warns, without stopping play, when:

- a layer is in the same space as the player start, so the player could fly into it: seal it with solid;
- a layer's position is in solid, so it shows nothing: move it into its room;
- a wall shows a View slot that has no layer: it shows a plain sky gradient;
- a mover has a View face: a mover does not draw views.

Two layers with the same slot and layer number: only the first counts. The Vista layer tool always starts at slot 0, layer 0, so set them in the panel.

# Texture Studio

Texture, Texture Studio makes materials from recipes: blocks of noise, bricks, blends and paint, with a height map for lighting. It covers the level while open; Back to the level or **Esc** returns.

- **New...** starts a material: a name of lower case letters, digits and - and a size from 64 to 1024.
- **Apply** puts it in the running editor, ready for the face panel. Nothing is written yet.
- **Save** writes the recipe and adds the material to the material list on disk. A material applied but not saved draws grey next session, and saving the level warns about it.
- While the studio is open, **Ctrl+Z**, **Ctrl+Y**, **Delete** and **Ctrl+S** act on the recipe, not the level, and Play is off.

The Texture Studio tutorial in the source tree, docs/texture-studio-tutorial.md, walks through a material from start to finish.

# Playing, saving and files

## Play (F5)

**F5**, **Ctrl+R** or Play, Play plays the level in the editor's window. The script is saved first. If the picture is still catching up, play starts when it is ready.

The player flies a ship in every direction. In play: **W A S D** move, **R F** up and down, the mouse or the arrows look, **Q E** roll, **Space** or the left button fires, **G** uses. **Esc** comes back, as does switching to another window. Playing never changes the level.

A level whose script is new must be saved before it can play, since the script lives in a file beside it: F5 opens Save as.

## Files

| File | What it holds |
|---|---|
| name.msl | The level, with its enemy kinds if it has its own |
| name.st | Its script |
| name.msl.lit | Its baked lighting, written on save |

- **Ctrl+S** saves the level, its script and its lighting. A level with no file yet asks for one.
- File, Open showcase level opens the demo level, which has no file until you save it.
- Saving checks the level as the loader will: a level the editor could not open again is not written, and the old file stays.
- New, Open and closing the window do not ask about unsaved level changes: watch for **[modified]** in the status bar. Unsaved script edits are asked about.

## Starting the editor

    mscent-editor level.msl
    mscent-editor --showcase
    mscent-editor --software

The first opens a level, the second the showcase, the third keeps the 3D view on the software renderer.

# Rules of a playable level

## To play at all

- At least one brush. An empty level: **Nothing to play: the level has no brushes**.
- A player start, not inside solid.
- A goal, unless the level has a script beside it: a script ends the level its own way, such as with an exit zone.
- No enemy inside solid.

These are checked when you press F5, and **Cannot play** says which failed.

## To build

- Every brush has a positive size and stays within -4096 to 4096.
- Every entity's values are in range.
- Names follow the naming rules and are not used twice.

A level that breaks one of these shows **Preview not updated**, and the Lighting log has the error.

## Good habits

- Draw rooms at Z = 0 and up, so entities placed from the Top view (at Z = 1.5) land inside them.
- Keep the outside solid: the void past the world's edge is solid, so a level cannot leak, but a room the player cannot reach still costs lighting.
- Keep vista rooms sealed: solid between them and the play space.
- A player start too close to a wall or floor can be in solid: keep it a unit away.

# Cookbook: a first room

1. File, New. Grid size 1.
2. **B**, New brush minus. In the Top view, drag from (-8, -8) to (8, 8). The room is 16 by 16 and 4 tall.
3. Select it with **Q** and set Max Z to 6 in the panel for a higher ceiling.
4. **T**, click the floor in the 3D view, and pick **floor** in the face panel. Ctrl + click the four walls and pick **brick**.
5. **P**, click near one wall in the Top view. Set Yaw so the arrow drawn in the Top view points into the room.
6. **L**, click in the middle. Set its Radius to 14, a little more than half the room.
7. **G**, click near the far wall.
8. **Ctrl+S**, then **F5**.

# Cookbook: rooms, doorways and pillars

## A second room and a doorway

1. Draw a second carved room beside the first, with solid between them.
2. Draw a narrow carved box through the wall between them: 2 wide, 3 tall, overlapping both rooms. That is a doorway.
3. For a corridor, make the doorway box long: a corridor is only a carved box joining two rooms.

## A pillar

1. Press the plus New brush button.
2. Draw a 2 by 2 box in the middle of the room, as tall as the room.
3. It comes last, so it stands. If it vanishes, it is earlier in the order than the room: Edit, Move later in order until it shows.

## Steps and ledges

Draw add boxes one above the other, each a little higher and shorter than the last, after the room. The player flies, so steps are scenery and cover rather than a way up: leave at least 1.5 units of headroom wherever the ship should pass.

## A ramp

1. Draw an add box where the ramp goes.
2. **V**, and in the Side or Front view drag the top corners at one end down to the floor.
3. The box becomes a convex wedge.

## An arch or a window

1. Draw the wall as an add box, or keep the solid between two rooms.
2. Draw a subtract box where the opening goes, after the wall.
3. For a rounded top, slice the opening's corners off with **C**: drag a line across a corner, **K** to keep the side inside the opening, **Enter**.

# Cookbook: lighting a room

- Put lights where a lamp would be: near the ceiling, against a wall. A light in the middle of a room lights it flat.
- Set Radius to reach the far wall. Beyond its radius a light adds nothing, so a radius too small leaves dark corners and one far too large costs bake time.
- Warm (1, 0.85, 0.6) and cool (0.6, 0.75, 1) lights in one room give the walls shape.
- Use a Projector pointing down (Pitch -90) for a pool of light under a ceiling lamp, or tilted onto a wall to pick out a door.
- Paint a ceiling face with **lightpanel** under a light, so the light has a visible source.
- Fog changes the mood more than any lamp: Edit, Level settings, Fog start 2 and Fog end 18 makes a tight, murky level.

# Cookbook: a door that opens

1. Carve two rooms joined by a doorway (see rooms, doorways and pillars).
2. Script, Add entity, Door. Click the doorway in the Top view.
3. The door fits the smallest carved box around the click, so the doorway box itself is the best place to click.
4. The script gets the door's lines, and the door and its area are named **door_1** and **door_1_area**.
5. **Ctrl+S**, then **F5**: the door opens as the player comes near.

For a door with a key, use Locked door; for one that opens when switches are shot, Switch door. The Script pane's guide (**F7**) explains the lines they add.

# Cookbook: a cave

1. Carve a rough room with a box, a little bigger than the cave.
2. **X**. In the Voxel panel set Dab to Carve, Radius 2, New cells 0.25.
3. Drag across the walls in the 3D view to dig into them. Ctrl + drag puts rock back.
4. Set Dab to Smooth and go over the rough parts.
5. Set Dab to Roughen and brush the walls lightly for texture.
6. Set Generate to Stalactites, Radius 1.5, and click the ceiling. Roll with the wheel until you like it, then Commit.
7. Light it with a few dim, warm lights low down, and play.

For a long passage, use Tunnel: click the mouth, then click along the way, and **Esc** at the end.

# Cookbook: a sky and distant mountains

1. Make a room with a high ceiling.
2. Edit, New vista. The status bar names the slot, such as **View 0**.
3. **T**, click the ceiling in the 3D view, and pick **View 0** at the end of the materials. The ceiling becomes a starry sky with a planet.
4. Edit, New mountains. A range stands in front of the stars.
5. Paint a wall **View 0** too: the mountains show through it on the horizon.
6. Watch the Lighting log for vista warnings.

# Techniques

- **Think in rooms.** Carve every space as a subtract box first, then add details with add boxes after them in the order.
- **Overlap, do not touch.** Rooms joined by a doorway box that overlaps both join cleanly; boxes that only touch leave a wall of zero thickness.
- **Coarse grid first.** Lay out at grid 2 or 4, then go down to 0.5 or 0.25 for detail.
- **Work in the Top view.** Draw rooms and place entities there; fix heights in the Front or Side view, or in the panel.
- **Carve with brush for openings.** Draw a box where a hole should go in an add brush and press Ctrl+Shift+C: the brush is cut, not hidden.
- **Wrap textures round corners.** Alt + click the wall you like, Alt + right click the next: the texture continues across the edge.
- **Texture moves with brush** off on walls that should line up across several brushes; on for doors and movers.
- **Sculpt after the layout.** A voxel volume sits in the brush order; carve its room with brushes first.
- **Commit generators.** A held result goes if you change tool.
- **Save often.** Saving writes the lighting too, so the level opens lit next time.

# Keys

## Tools

| Key | Tool |
|---|---|
| Q | Select / move |
| B | Box brush |
| C | Slice brush |
| V | Move corners |
| T | Face / texture |
| X | Voxel |
| P | Player start |
| L | Light |
| J | Projector |
| E | Enemy |
| G | Goal |
| M | Marker |
| Z | Zone |
| Y | Vista layer |

Tool keys do nothing while a text field has the keyboard. While the right button is held in the 3D view, Q and E roll the camera instead of picking Select and Enemy.

## Commands

| Key | Command |
|---|---|
| F1 | This manual, on the tool in hand |
| F5 or Ctrl+R | Play |
| F6 | Show or hide the Script pane |
| F7 | Scripting guide |
| Esc | Leave play, drop a slice line or a stroke, leave the Texture Studio, close this manual |
| Ctrl+N | New level |
| Ctrl+O | Open |
| Ctrl+S | Save |
| Ctrl+Z | Undo |
| Ctrl+Y or Ctrl+Shift+Z | Redo |
| Delete | Delete the selection |
| Enter | Apply the slice |
| K | Choose what the slice keeps |
| Ctrl+Shift+C | Carve with brush |
| Ctrl+E | Extrude picked faces out |
| Ctrl+Shift+E | Extrude picked faces in |

## Mouse

| Where | What |
|---|---|
| 2D view, left | The tool |
| 2D view, middle drag | Pan |
| 2D view, wheel | Zoom |
| 3D view, right drag | Look |
| 3D view, left | Select, pick a face or sculpt |
| 3D view, Ctrl + left | Add a face to the picks, or swap Add and Carve |
| 3D view, Alt + left | Copy a texture, or build up a stroke |
| 3D view, Alt + right | Paste a texture |
| 3D view, wheel | Voxel radius, or roll a generator again |

# Troubleshooting

## Messages

| Message | What to do |
|---|---|
| Brush must have a positive size | Max must be above Min on every axis. |
| Entity value out of range | The value is outside what the field allows: colours 0 to 1, sizes above 0. |
| A corner cannot go there | The move would make the shape concave, merge corners or leave the map. |
| The line does not cut through the brush | Draw the slice line right across the selected brush. Volumes cannot be sliced. |
| Select a brush to carve with | Select a box or convex brush. Volumes cannot carve. |
| The brush overlaps nothing to carve | Move it into what it should cut. |
| Pick faces with the Face tool to extrude | Extrude needs faces picked with T. |
| Names and tags are lower-case letters, digits and _, at most 32 | Fix the name. |
| That name is already taken | Every name in a level is different. |
| A mover must be an add brush | Flip it to add first. |
| Another volume is in the way | Volumes cannot overlap: move one, or sculpt in the other. |
| The volume has reached its largest size | Use larger cells, or start a second volume beside it. |
| Cannot play: the level has no player start | Place one with P. |
| Cannot play: the level has no goal | Place one with G, or add a script with an exit. |
| SpawnInSolid | The player start or an enemy is inside solid. Check its Z. |
| Save the level first: its script lives in a file beside it | Save, then play. |
| The script changed on disk | Something else edited the script: Save overwrites it, Reload on the Script pane takes it. |

## Nothing shows in the 3D view

- A level with no brushes shows nothing: draw a subtract box.
- An add brush with nothing carved before it shows nothing: carve a room first.
- The camera may be in solid: View, Reset 3D camera.

## A brush I drew disappeared

Something later in the order covers it. Select it and use Edit, Move later in order.

## An entity is not where I put it

It was placed at Z = 1.5 from the Top view, or at 0 from another view. Set its Pos in the panel.

## The light looks blotchy

That is draft lighting while the bake runs: wait for **Lighting up to date**.

## A vista shows only a gradient

The View slot painted on the wall has no layer, or the layer is in solid. Read the warnings in the Lighting log.

# Known rough edges

These behave in ways that can surprise you. Each is filed to be fixed; until then, here is how to work with it.

- **Unsaved levels are not asked about.** New, Open, the showcase, a sample and closing the window drop unsaved changes to the level. Save first when the status bar says **[modified]**. (Issue 63.)
- **The hidden coordinate.** New entities land at Z = 1.5 from the Top view, 0 from the others, and new boxes are 4 deep. Check heights in the Front view. (Issue 64.)
- **Fields refuse or clamp.** A light's colour past 1 is refused; a projector's angle past 90 is clamped. Type values rather than using the arrows. (Issue 65.)
- **Cryptic messages.** SpawnInSolid(N) counts entities from 0, other messages from 1. (Issue 66.)
- **Silent edits.** Reordering at either end, moving a volume into another, and a second player start say nothing. (Issue 67.)
- **Q and E in the 3D view** roll and also pick Select and Enemy, unless the right button is held. (Issue 68.)
- **Changing tool drops a held generator result.** Commit first. (Issue 69.)
- **The Vista layer tool always starts at slot 0, layer 0**, and the Lighting log shows only its newest lines. (Issue 70.)
