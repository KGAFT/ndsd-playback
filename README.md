# ndsd-playback
A lightweight library in rust for native dsd playback over compatible devices.

This library will support only dsd without pcm conversion, you will need dac with dsd support!

# Currently supported:


Feature | status
--- | --- 
dsf/dsdiff reading | supported
dsd playback | supported
metadata parsing | TODO

# Honorable mention

The current asio player works like shit. But works. Behavioural fix TODO.

Use at your own risk

# What will not be supported:

SACD iso images playback, due to obvious reasons.

# Maybe will be supported:

Android dsd playback

# Examples:

You can find example usage case in lib.rs test case