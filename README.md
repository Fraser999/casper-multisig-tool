# `casper-multisig-tool`

## Dependencies

See the [`fltk-rs` docs](https://github.com/fltk-rs/fltk-rs#dependencies) for full info.

### Debian
```
sudo apt install libx11-dev libxext-dev libxft-dev libxinerama-dev libxcursor-dev libxrender-dev libxfixes-dev libpango1.0-dev libpng-dev libgl1-mesa-dev libglu1-mesa-dev
```

### RHEL-based
```
sudo yum groupinstall "X Software Development" && yum install pango-devel libXinerama-devel libpng-devel
```

### Arch
```
sudo pacman -S libx11 libxext libxft libxinerama libxcursor libxrender libxfixes libpng pango cairo libgl mesa --needed
```

### Alpine
```
apk add pango-dev fontconfig-dev libxinerama-dev libxfixes-dev libxcursor-dev libpng-dev mesa-gl
```

### NixOS
```
nix-shell --packages rustc cmake git gcc xorg.libXext xorg.libXft xorg.libXinerama xorg.libXcursor xorg.libXrender xorg.libXfixes libpng libcerf pango cairo libGL mesa pkg-config
```

## To run

```console
cargo r --release
```
