The goal is to build an app dedicated to managing releases of ship of harkinian (https://github.com/HarbourMasters/Shipwright)

The short term goal is to accomplish the following:
- Be able to download and install any release
- Launch any installed version
- Specify a path to either or both of a required n64 rom (ocarina of time or ocarina of time master quest)
- direct the app to use the specified rom(s) when prompted

Long term, the app will also be able to do the following:
- Be usable in any of linux, macos, or windows.
- manage mods on any installed version
- and keep a library of mods which can be arbitrarily enabled/disabled for any installed version
- manage other harbourmasters projects https://github.com/HarbourMasters such as ghostship, starship, spaghettikart, mm, etc.

These long term features will not be built immediately, but the design should take these future needs into account and build around that.

The app should be built using rust. It should use os-native elements where possible. Any os-specific elements can be leveraged using apis and whatever tools are native to that language, such as swift, gtk, or whatever windows uses.

The first version of the app will be built for macos. Once basic functionality is established there, then linux will be added, followed by windows last.
