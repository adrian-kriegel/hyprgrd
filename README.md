# hyprgrd

A grid-based workspace switcher for Hyprland.

# TODO

  1. Touchpad gestures seem to be broken. I think the issue is that hyprgrd still assumes gestures.workspace_swipe exists which is no longer supported in favor of newer gesture configs. 
  2. When switching workspaces, the mouse cursor will sometimes focus a different display. It should still focus the display it focused before switching. 
  3. The "movego" command does not trigger the visualizer but should.
  4. The "movego" command will sometimes move the focused window to a different monitor.
  5. "movego" seems to be pretty unstable in general. 
  6. Add some way of attaching to the stdout of the daemon process when run through hyprland exec-once
  7. The visualize does not show on the focused display. This should be configurable in the config "show on focused display" and "show on all displays" 

## Sending commands

Connect to the Unix socket and write newline-delimited JSON. Each line is one command.

### Command reference

| Command | JSON | Effect |
|---|---|---|
| Go in a direction | `{"Go":"Right"}` | Move one grid cell right (also `Left`, `Up`, `Down`) |
| Switch to absolute position | `{"SwitchTo":{"x":2,"y":1}}` | Jump to column 2, row 1 (0-indexed) |
| Move window and go | `{"MoveWindowAndGo":"Left"}` | Carry the focused window one cell left |
| Move window to monitor | `{"MoveWindowToMonitor":"Right"}` | Move focused window to the monitor in the given direction |
| Move window to monitor N | `{"MoveWindowToMonitorIndex":1}` | Move focused window to monitor at index N (0-based) |
| Prepare move (gesture) | `{"PrepareMove":{"dx":0.5,"dy":-0.3}}` | Preview a partial move (for animations) |
| Cancel move | `"CancelMove"` | Snap the preview back (gesture too small) |
| Commit move | `{"CommitMove":"Down"}` | Finish a gesture and actually switch |
| Swipe begin | `{"SwipeBegin":{"fingers":3}}` | Start a raw touchpad swipe (sent by plugin) |
| Swipe update | `{"SwipeUpdate":{"fingers":3,"dx":10.5,"dy":-2.3}}` | Incremental finger delta in pixels (sent by plugin) |
| Swipe end | `"SwipeEnd"` | Fingers lifted — commit or cancel based on threshold |
| Toggle visualizer | `"ToggleVisualizer"` | Toggle a persistent overlay showing the current grid state without moving workspaces |

### Examples with socat

```sh
# Go right
echo '{"Go":"Right"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock

# Switch to grid position (2, 0)
echo '{"SwitchTo":{"x":2,"y":0}}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock

# Move the focused window down
echo '{"MoveWindowAndGo":"Down"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock

# Move the focused window to the monitor on the right
echo '{"MoveWindowToMonitor":"Right"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock

# Move the focused window to the second monitor (0-indexed)
echo '{"MoveWindowToMonitorIndex":1}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock
```

### Hyprland keybinds (with plugin)

The recommended way to bind keys is through the **hyprgrd Hyprland plugin**, which registers native dispatchers. This avoids spawning a shell or socat for every keypress — the plugin connects to the daemon socket directly inside the compositor process.

See [Plugin](#plugin) below for build/install instructions.

```conf
# Navigate the grid
bind = SUPER, right, hyprgrd:go,     right
bind = SUPER, left,  hyprgrd:go,     left
bind = SUPER, up,    hyprgrd:go,     up
bind = SUPER, down,  hyprgrd:go,     down

# Move focused window and follow
bind = SUPER SHIFT, right, hyprgrd:movego, right
bind = SUPER SHIFT, left,  hyprgrd:movego, left
bind = SUPER SHIFT, up,    hyprgrd:movego, up
bind = SUPER SHIFT, down,  hyprgrd:movego, down

# Move focused window to another monitor
bind = SUPER ALT, right, hyprgrd:movetomonitor, right
bind = SUPER ALT, left,  hyprgrd:movetomonitor, left
bind = SUPER ALT, up,    hyprgrd:movetomonitor, up
bind = SUPER ALT, down,  hyprgrd:movetomonitor, down

# Move focused window to monitor by index (0-based)
bind = SUPER ALT, 1, hyprgrd:movetomonitorindex, 0
bind = SUPER ALT, 2, hyprgrd:movetomonitorindex, 1

# Jump to absolute grid positions
bind = SUPER, 1, hyprgrd:switch, 0 0
bind = SUPER, 2, hyprgrd:switch, 1 0
bind = SUPER, 3, hyprgrd:switch, 2 0

# Toggle persistent visualizer overlay (for example, with Escape)
bind = , escape, hyprgrd:togglevis
```

The dispatchers can also be invoked from the command line:

```sh
hyprctl dispatch hyprgrd:go right
hyprctl dispatch hyprgrd:movego left
hyprctl dispatch hyprgrd:switch 2 1
hyprctl dispatch hyprgrd:movetomonitor right
hyprctl dispatch hyprgrd:movetomonitorindex 0
```

### Hyprland keybinds (home-manager)

If you manage your Hyprland config with [home-manager](https://github.com/nix-community/home-manager), you can declare the plugin and keybinds declaratively:

```nix
{ pkgs, hyprgrd, ... }:

{
  wayland.windowManager.hyprland = {
    enable = true;

    plugins = [
      hyprgrd.packages.${pkgs.system}.plugin
    ];

    settings = {
      bind = [
        # Navigate the grid
        "SUPER, right, hyprgrd:go, right"
        "SUPER, left,  hyprgrd:go, left"
        "SUPER, up,    hyprgrd:go, up"
        "SUPER, down,  hyprgrd:go, down"

        # Move focused window and follow
        "SUPER SHIFT, right, hyprgrd:movego, right"
        "SUPER SHIFT, left,  hyprgrd:movego, left"
        "SUPER SHIFT, up,    hyprgrd:movego, up"
        "SUPER SHIFT, down,  hyprgrd:movego, down"

        # Move focused window to another monitor
        "SUPER ALT, right, hyprgrd:movetomonitor, right"
        "SUPER ALT, left,  hyprgrd:movetomonitor, left"
        "SUPER ALT, up,    hyprgrd:movetomonitor, up"
        "SUPER ALT, down,  hyprgrd:movetomonitor, down"

        # Move focused window to monitor by index (0-based)
        "SUPER ALT, 1, hyprgrd:movetomonitorindex, 0"
        "SUPER ALT, 2, hyprgrd:movetomonitorindex, 1"

        # Jump to absolute grid positions
        "SUPER, 1, hyprgrd:switch, 0 0"
        "SUPER, 2, hyprgrd:switch, 1 0"
        "SUPER, 3, hyprgrd:switch, 2 0"
      ];
    };
  };
}
```

**If you use Hyprland from nixpkgs** (`programs.hyprland.enable = true`):

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    home-manager.url = "github:nix-community/home-manager";
    hyprgrd = {
      url = "github:your-user/hyprgrd";
      inputs.nixpkgs.follows = "nixpkgs";   # ← ensures same Hyprland version
    };
  };
}

### Hyprland keybinds (without plugin)

If you prefer not to use the plugin, you can send commands through socat (or any tool that writes to a Unix socket):

```conf
bind = SUPER, right, exec, echo '{"Go":"Right"}' | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock
bind = SUPER, left,  exec, echo '{"Go":"Left"}'  | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock
bind = SUPER, up,    exec, echo '{"Go":"Up"}'    | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock
bind = SUPER, down,  exec, echo '{"Go":"Down"}'  | socat - UNIX-CONNECT:$XDG_RUNTIME_DIR/hyprgrd.sock
```

## Touchpad gestures

Touchpad swipe gestures are forwarded by the **hyprgrd Hyprland plugin**. The plugin hooks into Hyprland's swipe pipeline, forwards the raw events to the daemon, and **cancels** the default workspace-swipe handling so Hyprland doesn't fight over the gesture.

### Setup

1. Build and load the [plugin](#plugin).
2. Enable Hyprland's gesture pipeline so swipe events are emitted (required for the plugin to receive hooks). **Hyprland 0.51+** uses the new `gesture = ...` syntax; the old `workspace_swipe = true` option was removed.

```conf
gestures {
  gesture = 3, horizontal, workspace
  gesture = 4, horizontal, workspace
}
```

Use both 3- and 4-finger horizontal workspace gestures so that both “switch workspace” (3 fingers) and “move window and go” (4 fingers) work. The plugin intercepts the events before Hyprland acts on them, so you won't see Hyprland's built-in workspace sliding animation.

### Gesture behaviour

| Gesture | Behaviour |
|---|---|
| 3-finger swipe | Preview the move in the grid overlay, then switch workspace on release or snap back if below threshold |
| 4-finger swipe | Same as above but carries the focused window along |

### Gesture configuration

The sensitivity and commit threshold can be tuned in `~/.config/hyprgrd/config.json`:

```json
{
  "gestures": {
    "sensitivity": 200.0,
    "commit_threshold": 0.3,
    "commit_while_dragging_threshold": 0.8,
    "switch_fingers": 3,
    "move_fingers": 4,
    "natural_swiping": true
  }
}
```

| Key | Default | Description |
|---|---|---|
| `sensitivity` | `200.0` | Pixels of finger travel per normalised unit (higher = less sensitive) |
| `commit_threshold` | `0.3` | Normalised distance the gesture must exceed on **release** to commit the switch |
| `commit_while_dragging_threshold` | *(none)* | If set (0.0–1.0), commit as soon as the gesture reaches this fraction toward the next cell, without waiting for release (e.g. `0.8` = switch at 80% of the way) |
| `switch_fingers` | `3` | Finger count for workspace switch gestures |
| `move_fingers` | `4` | Finger count for move-window-and-switch gestures |
| `natural_swiping` | `true` | Invert gesture direction (swipe right → grid moves left, like natural scroll) |

## Visualizer

When built with the default `visualizer-gtk` feature, hyprgrd displays a small grid overlay on every workspace switch. The overlay shows your position in the grid: a bright **sliding cursor** marks the current cell, and previously visited cells are dimly highlighted.

The cursor physically **glides** between cells with an ease-out animation on discrete navigation commands (`Go`, `CommitMove`, etc.). During a touchpad gesture the cursor tracks your finger in real time. After a switch the overlay lingers briefly, then **fades out** smoothly.

### Configuration

Create `~/.config/hyprgrd/config.json` (or `$XDG_CONFIG_HOME/hyprgrd/config.json`) to tune timing. All durations are in milliseconds. Every field is optional — omitted fields use the defaults shown below.

```json
{
  "visualizer": {
    "cursor_animation_ms": 80,
    "linger_ms": 300,
    "fade_out_ms": 200
  },
  "gestures": {
    "sensitivity": 200.0,
    "commit_threshold": 0.3,
    "commit_while_dragging_threshold": 0.8,
    "switch_fingers": 3,
    "move_fingers": 4,
    "natural_swiping": true
  }
}
```

| Key | Default | Description |
|---|---|---|
| `cursor_animation_ms` | `80` | Cursor slide duration (ease-out cubic) |
| `linger_ms` | `300` | Time the overlay stays fully opaque after a switch, before fading |
| `fade_out_ms` | `200` | Fade-out duration (set `0` for instant hide) |

See [Touchpad gestures → Gesture configuration](#gesture-configuration) for the `gestures` keys.

### Styling with CSS

Create `~/.config/hyprgrd/style.css` (or `$XDG_CONFIG_HOME/hyprgrd/style.css`) to customise the overlay. If the file does not exist, sensible built-in defaults are used.

#### Widget tree

```
window                         transparent layer-shell surface
└ .grid-overlay              dark rounded backdrop
    └ Overlay
        ├ .grid              GtkGrid holding all cells
        │   ├ .grid-cell             base (unvisited) cell
        │   └ .grid-cell.active      cell under the cursor
        └ .grid-cursor       the bright sliding selector
```

#### CSS selectors

| Selector | What it styles |
|---|---|
| `window` | The overlay window — keep `background-color: transparent` |
| `.grid-overlay` | The dark rounded container around the grid |
| `.grid-overlay.mode-auto` | Overlay while it is shown automatically for navigation / gestures (non-interactive by default) |
| `.grid-overlay.mode-manual` | Overlay while it is manually pinned open; grid cells are clickable to switch workspaces |
| `.grid` | The `GtkGrid` widget itself |
| `.grid-cell` | Every cell (base dim colour) |
| `.grid-cell.active` | The cell directly under the cursor (CSS hook for advanced styling) |
| `.grid-cursor` | The sliding highlight — this is the primary active indicator |

#### Default stylesheet

```css
window,
window.background {
    background-color: transparent;
    background: none;
}

.grid-overlay {
    background-color: rgba(0, 0, 0, 0.75);
    border-radius: 16px;
    padding: 12px;
}

.grid-cell {
    min-width: 24px;
    min-height: 24px;
    margin: 3px;
    border-radius: 6px;
    background-color: rgba(255, 255, 255, 0.08);
    transition: background-color 150ms ease-in-out;
}

.grid-cursor {
    background-color: rgba(255, 255, 255, 0.9);
    border-radius: 6px;
}

.grid-overlay.mode-manual,
.grid-overlay.mode-manual .grid-cell {
    cursor: pointer; /* indicate that cells are clickable when the visualizer is pinned open */
}
```

#### Examples

**Blue cursor with glow:**

```css
.grid-cursor {
    background-color: #7aa2f7;
    border-radius: 8px;
    box-shadow: 0 0 8px rgba(122, 162, 247, 0.6);
}
```

**Catppuccin-themed overlay:**

```css
.grid-overlay {
    background-color: rgba(30, 30, 46, 0.85);
}

.grid-cell {
    background-color: rgba(205, 214, 244, 0.06);
}

.grid-cursor {
    background-color: rgba(137, 180, 250, 0.9);
    border-radius: 8px;
}
```

**Larger cells:**

```css
.grid-cell {
    min-width: 32px;
    min-height: 32px;
    margin: 4px;
    border-radius: 8px;
}
```

> **Note:** The cursor slide, linger, and fade-out timings are controlled by `config.json`, not by CSS. The CSS `transition` property on `.grid-cell` is not respected.

### Debug overlay

A standalone test binary is included for verifying the overlay works without running the full daemon:

```sh
cargo run --bin hyprgrd-test-overlay
```

This shows a 3×3 grid with a cursor that slides clockwise around the cells automatically.

## Plugin

The `plugin/` directory contains a Hyprland C++ plugin that provides:

- **Five native dispatchers** (`hyprgrd:go`, `hyprgrd:movego`, `hyprgrd:switch`, `hyprgrd:movetomonitor`, `hyprgrd:movetomonitorindex`) — keybinds call the daemon directly from inside the compositor, no shell or socat needed.
- **Touchpad gesture forwarding** — hooks Hyprland's `swipeBegin` / `swipeUpdate` / `swipeEnd` events, forwards them to the daemon as raw swipe commands, and cancels the default workspace-swipe so Hyprland doesn't interfere.


### Dispatcher reference

| Dispatcher | Argument | Example |
|---|---|---|
| `hyprgrd:go` | `left` / `right` / `up` / `down` | `bind = SUPER, right, hyprgrd:go, right` |
| `hyprgrd:movego` | `left` / `right` / `up` / `down` | `bind = SUPER SHIFT, right, hyprgrd:movego, right` |
| `hyprgrd:switch` | `<col> <row>` (0-indexed, space-separated) | `bind = SUPER, 1, hyprgrd:switch, 0 0` |
| `hyprgrd:movetomonitor` | `left` / `right` / `up` / `down` | `bind = SUPER ALT, right, hyprgrd:movetomonitor, right` |
| `hyprgrd:movetomonitorindex` | `<n>` (0-based monitor index) | `bind = SUPER ALT, 1, hyprgrd:movetomonitorindex, 0` |
| `hyprgrd:togglevis` | *(no args)* | `bind = , escape, hyprgrd:togglevis` |


### Building the plugin

With Nix (recommended — pulls the correct Hyprland headers automatically):

```sh
nix build .#plugin
```

The resulting `hyprgrd.so` is in `result/lib/hyprland/`.

Or manually with CMake inside the dev shell:

```sh
nix develop
cmake -B plugin/build -S plugin
cmake --build plugin/build
```

### Loading the plugin

Add to your `hyprland.conf`:

```conf
plugin = /absolute/path/to/plugin/hyprgrd.so
```

Or load it at runtime:

```sh
hyprctl plugin load /absolute/path/to/plugin/hyprgrd.so
```

## Architecture

All core logic is decoupled from Hyprland through two traits:

- **`WindowManager`** — switch workspaces, move windows. The Hyprland implementation uses the `hyprland` crate's IPC (no shell commands).
- **`CommandSource`** — emit commands from any transport. Ships with a Unix socket listener; touchpad gestures arrive over the same socket, forwarded by the Hyprland plugin.

```
src/
├ command.rs              Command enum, Direction, shared types
├ grid.rs                 Dynamic grid with on-demand growth
├ traits.rs               WindowManager + CommandSource + VisualizerEvent
├ switcher.rs             GridSwitcher orchestrator (trait-generic)
├ hyprland/
│   ├ wm.rs               WindowManager impl via Hyprland IPC
│   └ gestures.rs         CommandSource impl reading socket2 swipe events
├ ipc/
│   └ listener.rs         CommandSource impl over a Unix stream socket
├ visualizer/
│   ├ mod.rs
│   └ gtk.rs              GTK4 + layer-shell overlay with animated cursor
├ bin/
│   └ test_overlay.rs     Standalone debug overlay binary
├ lib.rs
└ main.rs
plugin/
├ main.cpp                Hyprland C++ plugin (dispatchers + gesture hooks)
├ helpers.hpp             Pure helpers (string ops, JSON builders)
├ test_plugin.cpp         Unit tests for helpers
├ test_plugin_symbols.cpp Symbol export tests for the built .so
└ CMakeLists.txt
```
