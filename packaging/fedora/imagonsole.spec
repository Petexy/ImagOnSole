Name:           imagonsole
Version:        0.9.1
Release:        1%{?dist}
Summary:        A photo viewer in the LineXinBar design language, shown as Pictures

# This program, and the locked Rust dependency graph vendored into the source
# archive. Every crate offering a choice is taken under its permissive option:
# self_cell as Apache-2.0 rather than GPL-2.0-only, r-efi as MIT rather than
# LGPL-2.1-or-later. What is left after that choice is this list, and it is
# derived from the lock file rather than remembered.
License:        GPL-3.0-only AND Apache-2.0 AND MIT AND Apache-2.0 WITH LLVM-exception AND BSD-2-Clause AND BSD-3-Clause AND ISC AND MPL-2.0 AND Unicode-3.0 AND Unlicense AND Zlib AND 0BSD AND CDLA-Permissive-2.0
URL:            https://github.com/Petexy/imagonsole
Source0:        imagonsole-%{version}.tar.gz

ExclusiveArch:  x86_64 aarch64

# Cargo's release profile emits no DWARF, so find-debuginfo would produce an
# empty debugsourcefiles.list and rpmbuild would fail on it after the whole
# build. An archive submission wants real debuginfo instead: drop this, and
# with it the -Cdebuginfo=0 in %build that holds Fedora's own -Cdebuginfo=2 off,
# so the DWARF is built and packaged rather than built and binned.
%global debug_package %{nil}

BuildRequires:  cargo >= 1.90
BuildRequires:  rust >= 1.90
BuildRequires:  gcc
BuildRequires:  pkgconfig
BuildRequires:  desktop-file-utils
BuildRequires:  libappstream-glib
# The design language, as Rust sources. It is a build dependency and not a
# runtime one: `lxb-render` is a path dependency, so cargo compiles it into
# this binary and the finished program links no liblxb_*.so at all.
BuildRequires:  lxb-toolkit-devel >= 0.9.1
# What the program links outright, each asked for as a pkg-config name, which
# is what the Rust bindings look for: ALSA for the interface sounds, libudev
# for the game controllers and xkbcommon for the keyboard.
BuildRequires:  pkgconfig(alsa)
BuildRequires:  pkgconfig(libudev)
BuildRequires:  pkgconfig(xkbcommon)

# Opened by name at run time rather than linked, so rpm's automatic dependency
# generator cannot see it in the ELF.
Requires:       libglvnd-egl
# Choosing another folder is put to whatever chooser this desktop runs, through
# the portal. Without one the toolkit draws its own, so this is not required.
Recommends:     xdg-desktop-portal
# With a Vulkan driver present this draws through it; without one it falls back
# to EGL, so the loader is worth having and is not required.
Suggests:       vulkan-loader

%description
Shown as Pictures. A photo viewer drawn in the LineXinBar design language:
the same colours,
glass, motion and marks as the shell it was made for, and driven from a
controller, a keyboard and a pointer at once. It is an ordinary Wayland
application and runs under GNOME or Plasma as readily as under that shell.

A folder is a grid of pictures the light travels across. Opening one fills the
screen with the photograph itself, at its own resolution rather than a
thumbnail of it, and the pictures either side are read before they are asked
for. In the viewer a direction pans wherever the picture is larger than the
screen and steps to the next one where it is not, so nothing has to be switched
on and the picture itself says what the controls will do; one button walks the
zoom stops, and a wheel or a pad's triggers zoom by however much they are
given. Photographs are shown the way up the camera held them, and folders are
sorted the way somebody reads them: nine before ten.

%prep
%autosetup -n imagonsole-%{version}

%build
export RUSTUP_TOOLCHAIN=stable
export CARGO_TARGET_DIR=target
# Fedora exports its own %%{build_rustflags} into RUSTFLAGS before this runs, and
# they carry -Cdebuginfo=2 -Cstrip=none. RUSTFLAGS is appended after the release
# profile's own flags and wins, so every crate in the graph was generating full
# DWARF — and with %%global debug_package %%{nil} above, no package was ever made
# of it. -Cdebuginfo=0 last is what turns that back off. It is worth about
# 274 MiB of resident memory on the final rustc here, measured: 1572 MiB with
# the DWARF, 1298 MiB without.
export RUSTFLAGS="${RUSTFLAGS:-} -Cdebuginfo=0"

# And Cargo takes its job count from the core count alone, knowing nothing about
# how much memory the machine has to hold that many rustc at once. wgpu and naga
# are in this graph and thin LTO with one codegen unit is what the release
# profile asks for, so the count has to answer to memory as well. The sister
# repository's shell was killed by the kernel's OOM killer twice on an 8 GiB
# Apple M1 for want of exactly this.
#
# Arithmetic rather than %%limit_build, the Fedora macro meant for this, which
# swallowed the remainder of the script it was used in on Fedora Asahi.
build_jobs="%{_smp_build_ncpus}"
build_room="$(awk '/^MemTotal:/ { n = int($2 / 1024 / 2048); print (n < 1 ? 1 : n) }' /proc/meminfo 2>/dev/null || true)"
if [ -n "$build_room" ] && [ "$build_room" -lt "$build_jobs" ]; then
    build_jobs="$build_room"
fi
echo "building with $build_jobs of %{_smp_build_ncpus} jobs, for the memory this machine has"
cargo build --offline --locked --release -j"$build_jobs"

%install
export CARGO_TARGET_DIR=target
./packaging/install.sh \
    --destdir %{buildroot} \
    --prefix %{_prefix} \
    --target-dir target

%check
export RUSTUP_TOOLCHAIN=stable
export CARGO_TARGET_DIR=target
# The same two as %%build. The dev profile asks for full DWARF and this phase
# builds the graph a second time to get it, with no package made of it either;
# a failing test still names its file and line, which the panic carries rather
# than DWARF.
export RUSTFLAGS="${RUSTFLAGS:-} -Cdebuginfo=0"
build_jobs="%{_smp_build_ncpus}"
build_room="$(awk '/^MemTotal:/ { n = int($2 / 1024 / 2048); print (n < 1 ? 1 : n) }' /proc/meminfo 2>/dev/null || true)"
if [ -n "$build_room" ] && [ "$build_room" -lt "$build_jobs" ]; then
    build_jobs="$build_room"
fi
cargo test --offline --locked -j"$build_jobs"
# The two files that are read by something other than this program. Both are
# installed by then, so what is checked is what ships rather than what is in
# the checkout.
desktop-file-validate %{buildroot}%{_datadir}/applications/imagonsole.desktop
appstream-util validate-relax --nonet \
    %{buildroot}%{_metainfodir}/io.github.petexy.imagonsole.metainfo.xml

%files
%license LICENSE
%doc README.md
%{_bindir}/imagonsole
%{_datadir}/applications/imagonsole.desktop
%{_datadir}/icons/hicolor/scalable/apps/imagonsole.svg
%{_metainfodir}/io.github.petexy.imagonsole.metainfo.xml

%changelog
* Thu Sep 24 2026 Piotr Lewandowski <piotr.petexy@gmail.com> - 0.9.1-1
- Released with LineXinBar 0.9.1. Ten languages, with the month and the clock
  written the way each of them writes them.
- Stepping through a folder crosses to the next picture half again as fast, on
  a clock of its own rather than the one a panel slides in on.
- The wallpaper carries the shell's sparkles and follows Theme > Particles.
- A flake at the root, so the Nix target has something to build; a demo folder
  of photographs to photograph; and package builds that say what they lack.
- Requires lxb-toolkit 0.9.1 to build: Ui::begin takes the particles argument
  there, and not in 0.9.0.

* Mon Aug 31 2026 Piotr Lewandowski <piotr.petexy@gmail.com> - 0.9.0-1
- First packaged release. A folder of pictures and one picture at a time, in
  the LineXinBar design language.
- The photograph is drawn at its own resolution rather than through the
  toolkit's 512-pixel thumbnail atlas, which is what a grid card wants and a
  photo viewer cannot use.
- Driven from a controller, a keyboard and a pointer at once: a direction pans
  or steps depending on the picture, one button walks the zoom stops, and the
  wheel and the pad's triggers zoom by however much they are given.
- Requires lxb-toolkit 0.9.0 to build, the version LineXinBar, the toolkit,
  DistriBumpy and CEDM all release under.
