# ndsd-playback
A lightweight library in rust for native dsd playback over compatible devices.

This library will support only dsd without pcm conversion, you will need dac with dsd support!

# Currently supported:


Feature | status
--- | --- 
dsf/dsdiff reading | supported
dsd playback | supported**
metadata parsing | TODO

** - Only alsa playback, working on asio version to be compatible with windows
# What will not be supported:

SACD iso images playback, due to obvious reasons.

# Maybe will be supported:

Android dsd playback

# Examples:

You can find example usage case in lib.rs test case