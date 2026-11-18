#!/usr/bin/env bash
# Builds Aknan Download's custom LGPLv3 ffmpeg.exe for 64-bit Windows:
# video/audio remuxing, preview, MP3 (LAME), clip/burn H.264 re-encode (OpenH264),
# AV1 decoding (dav1d), and full UTF-8 / Arabic RTL subtitle burn-in (libass + HarfBuzz + FriBidi + FreeType + DirectWrite).
#
#   tools/ffmpeg/build.sh <ffmpeg> <lame> <openh264> <dav1d> <freetype> <fribidi> <harfbuzz> <libass> <out>
set -euo pipefail
FF_SRC=$(realpath "$1"); LAME_SRC=$(realpath "$2"); H264_SRC=$(realpath "$3")
DAV1D_SRC=$(realpath "$4"); FT_SRC=$(realpath "$5"); FRI_SRC=$(realpath "$6")
HB_SRC=$(realpath "$7"); ASS_SRC=$(realpath "$8"); OUT=$(realpath -m "$9")
HOST=x86_64-w64-mingw32
JOBS=$(nproc 2>/dev/null || echo 4)
WORK=$(mktemp -d)
PREFIX="$WORK/prefix"
mkdir -p "$PREFIX" "$OUT"
CC=${CC:-$(command -v $HOST-gcc-posix || command -v $HOST-gcc || echo $HOST-clang)}
CXX=${CXX:-$(command -v $HOST-g++-posix || command -v $HOST-g++ || echo $HOST-clang++)}
export CFLAGS="-O2 -fno-strict-aliasing"
export CXXFLAGS="-O2 -fno-strict-aliasing"
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig" PKG_CONFIG_PATH=""

cat >"$WORK/cross.ini" <<EOF
[binaries]
c = '$CC'
cpp = '$CXX'
ar = '$HOST-ar'
strip = '$HOST-strip'
windres = '$HOST-windres'
pkg-config = 'pkg-config'

[built-in options]
default_library = 'static'
prefer_static = true

[host_machine]
system = 'windows'
cpu_family = 'x86_64'
cpu = 'x86_64'
endian = 'little'
EOF

echo "== LAME"
cp -r "$LAME_SRC" "$WORK/lame"
cd "$WORK/lame"
./configure --host=$HOST --prefix="$PREFIX" --enable-static --disable-shared \
  --disable-frontend --disable-decoder --disable-gtktest --enable-nasm=no CC="$CC" >/dev/null
make -j"$JOBS" >/dev/null 2>&1
make install >/dev/null

echo "== OpenH264"
cp -r "$H264_SRC" "$WORK/openh264"
cd "$WORK/openh264"
make -j"$JOBS" OS=mingw_nt ARCH=x86_64 CC="$CC" CXX="$CXX" AR=$HOST-ar \
  PREFIX="$PREFIX" install-static >/dev/null 2>&1 || { echo "OpenH264 failed"; exit 1; }

echo "== dav1d"
meson setup "$WORK/dav1d" "$DAV1D_SRC" --cross-file "$WORK/cross.ini" --prefix="$PREFIX" \
  --buildtype=release -Denable_tools=false -Denable_tests=false -Denable_examples=false >/dev/null
ninja -C "$WORK/dav1d" -j"$JOBS" >/dev/null
ninja -C "$WORK/dav1d" install >/dev/null

echo "== FreeType"
meson setup "$WORK/ft" "$FT_SRC" --cross-file "$WORK/cross.ini" --prefix="$PREFIX" \
  --buildtype=release -Dzlib=disabled -Dbzip2=disabled -Dpng=disabled -Dharfbuzz=disabled -Dbrotli=disabled >/dev/null
ninja -C "$WORK/ft" -j"$JOBS" >/dev/null
ninja -C "$WORK/ft" install >/dev/null

echo "== FriBidi"
meson setup "$WORK/fribidi" "$FRI_SRC" --cross-file "$WORK/cross.ini" --prefix="$PREFIX" \
  --buildtype=release -Ddocs=false -Dbin=false -Dtests=false -Dc_args="-O2 -DFRIBIDI_LIB_STATIC" >/dev/null
ninja -C "$WORK/fribidi" -j"$JOBS" >/dev/null
ninja -C "$WORK/fribidi" install >/dev/null

echo "== HarfBuzz"
meson setup "$WORK/hb" "$HB_SRC" --cross-file "$WORK/cross.ini" --prefix="$PREFIX" \
  --buildtype=release -Dfreetype=enabled -Dglib=disabled -Dgobject=disabled -Dcairo=disabled \
  -Dicu=disabled -Dtests=disabled -Dintrospection=disabled -Ddocs=disabled -Dutilities=disabled \
  -Dgdi=enabled -Ddirectwrite=enabled >/dev/null
ninja -C "$WORK/hb" -j"$JOBS" >/dev/null
ninja -C "$WORK/hb" install >/dev/null

echo "== libass"
meson setup "$WORK/ass" "$ASS_SRC" --cross-file "$WORK/cross.ini" --prefix="$PREFIX" \
  --buildtype=release -Dfontconfig=disabled -Ddirectwrite=enabled -Dcoretext=disabled \
  -Dlibunibreak=disabled -Dtest=false -Dprofile=false -Dc_args="-O2 -DFRIBIDI_LIB_STATIC" >/dev/null
ninja -C "$WORK/ass" -j"$JOBS" >/dev/null
ninja -C "$WORK/ass" install >/dev/null

echo "== ffmpeg"
mkdir -p "$WORK/ff"
cd "$WORK/ff"
"$FF_SRC/configure" \
  --prefix="$PREFIX" --target-os=mingw32 --arch=x86_64 --enable-cross-compile \
  --cross-prefix=$HOST- --cc="$CC" --cxx="$CXX" \
  --pkg-config=pkg-config --pkg-config-flags=--static \
  --extra-cflags="-I$PREFIX/include -DFRIBIDI_LIB_STATIC" \
  --extra-ldflags="-L$PREFIX/lib -static -Wl,--dynamicbase -Wl,--nxcompat -Wl,--high-entropy-va" \
  --extra-libs="-lstdc++ -ldwrite -lgdi32 -lusp10 -lrpcrt4" \
  --disable-everything --disable-autodetect --disable-doc \
  --disable-ffplay --disable-ffprobe --disable-debug \
  --disable-avdevice --disable-hwaccels --disable-iconv --disable-zlib --disable-bzlib --disable-lzma \
  --enable-static --disable-shared --enable-version3 \
  --enable-libmp3lame --enable-libopenh264 --enable-libdav1d \
  --enable-libfreetype --enable-libfribidi --enable-libharfbuzz --enable-libass \
  --enable-network --enable-protocol=file,subfile,pipe,http,tcp \
  --enable-demuxer=mov,matroska,mpegts,aac,mp3,ogg,flac,wav,ac3,eac3,webvtt,srt,ass,rawvideo \
  --enable-muxer=mp4,ipod,mov,matroska,webm,mp3,mpegts,adts,ogg,opus \
  --enable-decoder=aac,aac_latm,opus,vorbis,mp3,mp3float,flac,ac3,eac3,pcm_s16le,pcm_s24le,pcm_f32le,alac,h264,hevc,vp8,vp9,mpeg4,libdav1d,subrip,ass,ssa,webvtt,movtext,text,rawvideo \
  --enable-encoder=libmp3lame,libopenh264,aac \
  --enable-parser=h264,hevc,av1,vp8,vp9,aac,aac_latm,opus,vorbis,mpegaudio,flac,ac3 \
  --enable-bsf=aac_adtstoasc,h264_mp4toannexb,hevc_mp4toannexb,extract_extradata,vp9_superframe,av1_frame_merge,av1_frame_split,dump_extradata,setts \
  --enable-filter=aresample,aformat,anull,null,format,abuffer,abuffersink,buffer,buffersink,atrim,trim,volume,scale,setpts,asetpts,subtitles,ass,overlay \
  --enable-swresample --enable-swscale >"$WORK/configure.log" || { tail -40 "$WORK/configure.log"; exit 1; }
make -j"$JOBS" ffmpeg.exe >"$WORK/make.log" 2>&1 || { tail -40 "$WORK/make.log"; exit 1; }
$HOST-strip -o "$OUT/ffmpeg.exe" ffmpeg.exe
cp "$FF_SRC/COPYING.LGPLv3" "$OUT/ffmpeg-LICENSE.txt"
cp "$LAME_SRC/COPYING" "$OUT/lame-COPYING.txt"
cp "$H264_SRC/LICENSE" "$OUT/openh264-LICENSE.txt"
cp "$DAV1D_SRC/COPYING" "$OUT/dav1d-COPYING.txt"
cp "$FT_SRC/LICENSE.TXT" "$OUT/freetype-LICENSE.txt"
cp "$FRI_SRC/COPYING" "$OUT/fribidi-COPYING.txt"
cp "$HB_SRC/COPYING" "$OUT/harfbuzz-COPYING.txt"
cp "$ASS_SRC/COPYING" "$OUT/libass-COPYING.txt"
sed -n '/^License:/,$p' "$WORK/configure.log" | head -3 >"$OUT/ffmpeg-build.txt" || true
echo "built $(du -h "$OUT/ffmpeg.exe" | cut -f1)"
rm -rf "$WORK"
