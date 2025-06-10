+++
title = "An End To Terminal ANSI Codes"
date = 2025-05-12
authors = ["Tom Buckley-Houston"]
+++

What would a new terminal protocol without ANSI codes look like?

There are many ways to think about how the inner workings of modern terminal protocols could be improved. But just for the sake of argument and to keep things simple, let's imagine doing nothing more than deprecating ANSI all codes and letting CLI applications manipulate rendering details via an entirely new channel.

<!-- more -->

## What Are ANSI Codes?

The most common ANSI code is for rendering colour. So whereas: <nobr>`echo "Hello World"`</nobr> appears as normal text, the following will make the output a nice orange:

`echo -e "\033[38;2;255;100;0mHello World\033[0m"`

For comparison this is how we render coloured text in HTML:

```html
<span styles="color: #02ff64">Hello World</span>
```

Whilst it's slightly more verbose, it's notably easier to understand. And this is only the tip of the iceberg in terms of how deep and complex ANSI codes are. They control everything from text styling like colour, underlining, etc to moving the cursor and switching the terminal between archaic behaviour modes.

What makes matters even worse is that there is no universal standard. Terminals have and do support various subsets, supersets and variations of common ANSI codes. It is widely agreed that the current state of terminal protocols is generally a mess.

Gregory Sanders, a Neovim core contributor, has a great artlicle from 2024, [_State of the Terminal_](https://gpanders.com/blog/state-of-the-terminal/), giving a more in depth summary of ANSI codes and their history.  

## How Did We Get Here?

It's always sobering to remember that the very first computers didn't even have screens. They had what is called *teletypers*. These were basically typewriters that could be controlled electronically. So much of the foundations of computer science were forged on these machines. It's understandable that we still feel their legacy today.

A typewriter can only write text and spaces. It cannot delete letters nor easily go back to previous positions. Whilst this might seem like a limitation these days it has the advantage of being easy to implement. There's no need to add the overhead of graphical user interface. But whilst it may be borne of another era, the model, of inputting text commands and receiving "typed" output, is still applicable to the majority of tasks we perform in modern terminal emulators.

For all its ancient baggage it mostly just works. Which is more than can be said for a depressing amount of contemporary graphical environments. Terminals never really broke, and as such layers of legacy protocols have built up upon each other like strata of archaeological history.

## Terminal Sanctity

There's also another factor to consider when reflecting on improving terminals. We stand on the shoulders of giants. Terminals have their roots in a time when computers didn't rule the world, when decisions were based more on technical merit rather than their chance of increasing shareholder profits. Our little black screens are one of the few remaining digital spaces where the original love of computers for computers sake can flourish. Therefore suggesting new ways for how terminals work is not just a technical issue but a cultural one. The last thing we want is something like the browser wars, where corporations monopolise standards for nothing more than economic gain.

Terminals ain't broke, so why fix them?

## A Space To Play

So rather than suggest a new paradigm for terminals, what if we had a playground where we could easily explore new ideas? A tool whose job wasn't to *introduce* a sparkly new architecture, but whose job was to make playing with new architectures practical. Think of how `xwayland` works as a seamless interface between 2 worlds. Users largely don't need to worry about the implementation differences between X11 and Wayland. It's all just a graphical user interface at the end of day. Paradigm shifting changes have been made under the hood, but they mostly only effect developers (for better or worse).

I would like `tattoy` to be such a tool. It will always be a project for making terminals visually more appealing, so it has nothing to lose if the more lofty goals of evolving terminal internals never come to pass. As just one of its many features it can inconsequentially ship support for various new ways of running terminals¹. And it can do so in a way that is completely transparent to end users. If any one particular approach were to become popular, only then would it be refactored out into its own project and maybe even one day become a standard.

## Sending Colour To A Socket

So let's explore an idea. What if CLI applications could no longer use ANSI codes? So that all their output over STDOUT was literally rendered, nothing was parsed for styling and there was never any need for other programs that consume their output to worry about non-consumable data. Sounds good, but then how would text be styled or positioned? What if we sent this rendering information to a socket? The CLI program would have both the socket location of a rendering server and an ENV variable for the terminal instance it was currently a child of. The socket would have a well-known messaging format, `messagepack`, `protobuf`, `JSON` or even `HTML`, anything would be better than ANSI. Crucially the rendering server could still output readline-like content that automatically flowed and wrapped text as we've come to be accustomed. But it could also render styled text at arbitrary coordinates (without moving the cursor).

The socket would also respond to queries about the contents of the terminal and its scrollback. And could provide a unified and consistent protocol for accepting user input, most notably key combinations and mouse input would no longer be communicated via ANSI codes.

There would no longer be any need for arcane TTY modes like the so-called "alternate screen". CLI applications wouldn't need to query the `terminfo` database to know what ANSI codes are supported and if they are supported what syntax they should take.

It could even work over SSH with the tunnel argument `-L` that proxies the local rendering server's socket to the remote terminal.

## Tattoys

"Tattoy" is a mix of three words; "tattoo" for their visual appeal, "TTY" because it's a terminal app and "toy" because it's playful. So *tattoys* can take many forms, from superficial gimmicks to reimagined architectures. What they all share in common is playfulness, they don't have to be right or wrong, just engaging for certain people at certain moments.

Laboratories and incubators are similar, they're places to experiment and nurture. Whilst they don't outright encourage failures, they make them more acceptable, which in turn inspires creativity and the free flow of new ideas that have a meaningful place in the world.

## Notes
1. A notable serious attempt to evolve terminals is from the [Arcan](https://arcan-fe.com/about) project. It has a larger goal of experimenting with operating systems, but it has spent considerable time exploring terminals in particular, see: [Sunsetting Cursed Terminal Emulation](https://arcan-fe.com/2025/01/27/sunsetting-cursed-terminal-emulation/) for example.
