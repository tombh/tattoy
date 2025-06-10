+++
title = "Shaders"
template = "docs.html"
[extra]
weight = 4
+++

## Background

Shaders are written in the classic `glsl` language and are architected based on the shaders used by the well-known [Shadertoy](https://www.shadertoy.com/) website.

Here is a simple shader that renders a flowing gradient:
```glsl
void mainImage( out vec4 fragColor, in vec2 fragCoord )
{
    // Normalized pixel coordinates (from 0 to 1)
    vec2 uv = fragCoord/iResolution.xy;

    // Time varying pixel color
    vec3 col = 0.5 + 0.5*cos(iTime+uv.xyx+vec3(0,2,4));

    // Output to screen
    fragColor = vec4(col,1.0);
}
```

There are many great tutorials for getting you into Shadertoy and shaders in general. [Here's one to get you started](https://agatedragon.blog/2024/01/14/shadertoy-introduction/).

## Running A Shader

Tattoy comes with a default shader (`soft_shadows.glsl`). All you need to do to enable it is set `enabled = true` in the `[shader]` section of your [config file](/docs/config).

If you have more than one shader in your `shaders/` directory you can easily cycle through them using the following keybindings: `ALT-9`, `ALT-0`.

## Available Variables

Just like Shadertoy, Tattoy supports the following variables:
```glsl
vec3 iResolution;
vec2 iMouse;
float iTime;
int iFrame;
```

And a unique variable, `vec2 iCursor`, see [below](#icursor) for more details.

## Differences from Shadertoy

Tattoy supports most, but not all, of the shaders you'll find on Shadertoy. What Tattoy doesn't support:

* Multiple buffers. Buffers are extra shader files that are visible as UI tabs above the Shadertoy editor. 
* iChannels. These are found in the boxes below the Shadertoy editor.

### `iChannel0`
However Tattoy does have one special iChannel that you can reference in your shaders. Namely, `iChannel0` which contains a pixelated version of the current terminal contents. Each terminal cell is converted into two pixels, one that represents the top of the cell and the other the bottom. You can access these pixel colors like so:

```glsl
vec2 uv = vec2(terminal_x, terminal_y) / iResolution.xy;
vec3 color = texture(iChannel0, uv).rgb;
```

### `iCursor`

Just like Shadertoy, you can access the position of the mouse with `iMouse`. However, Tattoy also provides a similar variable named, `iCursor`, which stores the current `vec2` coordinates of the terminal's cursor. Both `iMouse` and `iCursor` are in the coordinate system of the terminal itself, with the exception that the y-axis is multiplied by 2. This is because a shader can actually render two "pixels" per terminal cell using the UTF8 half-block trick: "▀", "▄".

## Ghostty Shaders
Tattoy supports all [Ghostty](https://ghostty.org) shaders, for example those from the [ghostty-shaders repo](https://github.com/hackr-sh/ghostty-shaders). However, unlike Ghosty, Tattoy cannot affect font rendering. So for example shaders that distort the screen to create old school CRT effects, won't actually change the position or shape of any rendered text. The shaders still work but their impact isn't so pronounced.
