# ndsd-playback
A lightweight library in rust for native dsd playback over compatible devices.

This library will support only dsd without pcm conversion, you will need dac with dsd support!

# Currently supported:


Feature | status
--- | --- 
dsf/dsdiff * reading | supported
dsd playback | supported
metadata parsing | TODO


* -- dsdiff(dff) supports decompression dst*, only in mode dst64, dst128/dst256 is unstable. Base dsd streams works without issues
* -- dst decompressions uses parts of sacd foobar extension, builds with c++, you must enable it with features(dstdec)

# What will not be supported:

SACD iso images playback, due to obvious reasons.

# Maybe will be supported:

Android dsd playback

# Examples:

You can find example usage case in lib.rs test case

# If you struggle to build on windows

Modify the existing visual studio installation to support desktop development and linux one

Install the LLVM prebuild binaries, download it from the llvm project github repo.
Set LIBCLANG_PATH system environment variable pointing to the root of llvm/bin. E.g: C:\clang+llvm-22.1.0-x86_64-pc-windows-msvc\bin.
Also pass this directory to the system PATH variable.

If you have problems with function ASIOSetSampleRate and ASIOGetSampleRate it is an msvc bug.
Download and install ASIO SDK to your windows machine
Create the environment variable in the system space/userspace "CPAL_ASIO_DIR" pointing to the root of sdk

Go to your asio sdk, find asio.h and replace this:

```
#if IEEE754_64FLOAT
	typedef double ASIOSampleRate;
#else
	typedef struct ASIOSampleRate {
		char ieee[8];
	} ASIOSampleRate;
#endif
```

with this:

```
typedef double ASIOSampleRate;
```