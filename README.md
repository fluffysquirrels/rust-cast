[![Docs](https://docs.rs/rust_cast/badge.svg)](https://docs.rs/crate/rust_cast/)
![Build Status](https://github.com/azasypkin/rust-cast/actions/workflows/ci.yml/badge.svg)

# `rust_cast`: Rust crate for Chromecast

## Usage
* [Documentation](https://docs.rs/crate/rust_cast/)
* Try out [Rust Caster](./examples/rust_caster.rs) example to see this crate in action!

## Build

Proto files are taken from [Chromium Open Screen GitHub mirror](https://chromium.googlesource.com/openscreen/+/8cce349b0a595ddf7178d5730e980ace3a1d1a53/cast/common/channel/proto).

By default `cargo build` won't try to generate Rust code from the files located at `protobuf/*`, if you want to do that
use `GENERATE_PROTO` environment variable during build and make sure you have `protoc` binary in `$PATH`:

```bash
$ GENERATE_PROTO=true cargo build
```

## Run example

### Generic features

First, you need to figure out the address of the device to connect to. For example, you can use `avahi` with the following command:
```bash
$ avahi-browse -a --resolve
```

```bash
// Get some info about the Google Cast enabled device (e.g. Chromecast). 
$ cargo run --example rust_caster -- -a 192.168.0.100 -i

Number of apps run: 1
App#0: Default Media Receiver (CC1AD845)
Volume level: 1
Muted: false

// Run specific app on the Chromecast.
$ cargo run --example rust_caster -- -a 192.168.0.100 -r youtube

// Stop specific active app.
$ cargo run --example rust_caster -- -a 192.168.0.100 -s youtube

// Stop currently active app.
$ cargo run --example rust_caster -- -a 192.168.0.100 --stop-current

The following app has been stopped: Default Media Receiver (CC1AD845)
```

### Media features
```bash
// Stream a video.
$ cargo run --example rust_caster -- -a 192.168.0.100 -m http://commondatastorage.googleapis.com/gtv-videos-bucket/sample/BigBuckBunny.mp4

// Stream a video of specific format with buffering.
$ cargo run --example rust_caster -- -a 192.168.0.100 -m http://xxx.webm --media-type video/webm --media-stream-type buffered

// Stream video from YouTube (doesn't work with the latest YouTube app, fix is welcome).
$ cargo run --example rust_caster -- -a 192.168.0.100 -m 7LcUOEP7Brc --media-app youtube

// Display an image.
$ cargo run --example rust_caster -- -a 192.168.0.100 -m https://azasypkin.github.io/style-my-image/images/mozilla.jpg

// Change volume level.
$ cargo run --example rust_caster -- -a 192.168.0.100 --media-volume 0.5

// Mute/unmute media.
$ cargo run --example rust_caster -- -a 192.168.0.100 --media-mute [--media-unmute]

// Pause media.
$ cargo run --example rust_caster -- -a 192.168.0.100 --media-app youtube --media-pause

// Resume/play media.
$ cargo run --example rust_caster -- -a 192.168.0.100 --media-app youtube --media-play

// Seek media.
$ cargo run --example rust_caster -- -a 192.168.0.100 --media-app youtube --media-seek 100
```

For all possible values of `--media-type` see [Supported Media for Google Cast](https://developers.google.com/cast/docs/media).

## DNS TXT Record description

* `md` - Model Name (e.g. "Chromecast");
* `id` - UUID without hyphens of the particular device (e.g. xx12x3x456xx789xx01xx234x56789x0);
* `fn` - Friendly Name of the device (e.g. "Living Room");
* `rs` - Unknown (recent share???) (e.g. "Youtube TV");
* `bs` - Unknown (e.g. "XX1XXX2X3456");
* `st` - Unknown (e.g. "1");
* `ca` - Unknown (e.g. "1234");
* `ic` - Icon path (e.g. "/setup/icon.png");
* `ve` - Version (e.g. "04").

## Model names

* `Chromecast` - Regular chromecast, supports video/audio;
* `Chromecast Audio` - Chromecast Audio device, supports only audio.

## Useful links and sources of inspiration

### Official Google Chromecast docs
* [Cast overview](https://developers.google.com/cast/docs/overview)
* [Cast reference root](https://developers.google.com/cast/docs/reference)
* [Web sender overview](https://developers.google.com/cast/docs/web_sender)
* [Web sender API reference](https://developers.google.com/cast/docs/reference/web_sender)
* [Web receiver overview](https://developers.google.com/cast/docs/web_receiver)
* [Web receiver API reference](https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages)
* [Web receiver API - MessageType](https://developers.google.com/cast/docs/reference/web_receiver/cast.framework.messages#.MessageType)
  (the best reference for the possible cast message types and their JSON payloads)

* [Additional reference for messages in the media namespace](https://developers.google.com/cast/docs/media/messages)
  (a subset of the messages documented in the sender and receiver references, but does document the MetadataType enum's integer values)

### Open source Chromecast libraries
* [npm package `castv2`: an implementation of the Chromecast CASTV2 protocol in JS](https://github.com/thibauts/node-castv2);
    * [npm package `castv2-client`: a higher level wrapper for `castv2`](https://github.com/thibauts/node-castv2-client);
* [Chromecast - steps closer to a python native api](http://www.clift.org/fred/chromecast-steps-closer-to-a-python-native-api.html);
* [Hackernoon article: The Chromecast Protocol - A Brief Look](https://hackernoon.com/the-chromecast-protocol-a-brief-look) by GNOME GSoc 2022 Intern `@kyteinsky`
    * [Original post](https://kyteinsky.github.io/p/chromecast-protocol/) on `@kyteinsky`'s blog

### Miscellaneous
* [`openscreen` source in Chromium codebase](https://source.chromium.org/chromium/chromium/src/+/main:third_party/openscreen/;bpv=0;bpt=0)
* [`chrome_cast.js` web sender type reference in old version of the Chromium codebase](https://github.com/chromium/chromium/blob/105.0.5195.148/ui/file_manager/file_manager/externs/chrome_cast.js)
* [DIAL Protocol](http://www.dial-multiscreen.org/)
