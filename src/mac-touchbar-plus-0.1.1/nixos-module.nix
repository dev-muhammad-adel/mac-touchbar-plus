{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.services.tiny-dfr;
  
  # Default configuration
  defaultConfig = {
    enable = false;
    package = pkgs.tiny-dfr;
    user = "tiny-dfr";
    group = "tiny-dfr";
    extraGroups = [ "input" "video" ];
  };
  
  # Configuration file
  configFile = pkgs.writeText "tiny-dfr-config.json" (builtins.toJSON {
    # Add your configuration options here
    # This will be merged with the default config.json
  });
  
in {
  options.services.tiny-dfr = {
    enable = mkEnableOption "Enable tiny-dfr service";
    
    package = mkOption {
      type = types.package;
      default = pkgs.tiny-dfr;
      description = "The tiny-dfr package to use.";
    };
    
    user = mkOption {
      type = types.str;
      default = "tiny-dfr";
      description = "User account under which tiny-dfr runs.";
    };
    
    group = mkOption {
      type = types.str;
      default = "tiny-dfr";
      description = "Group under which tiny-dfr runs.";
    };
    
    extraGroups = mkOption {
      type = types.listOf types.str;
      default = [ "input" "video" ];
      description = "Extra groups that the tiny-dfr user should be added to.";
    };
  };
  
  config = mkIf cfg.enable {
    # Create user and group
    users.users = mkIf (cfg.user == "tiny-dfr") {
      tiny-dfr = {
        isSystemUser = true;
        group = cfg.group;
        extraGroups = cfg.extraGroups;
        description = "tiny-dfr service user";
        home = "/var/lib/tiny-dfr";
        createHome = true;
      };
    };
    
    users.groups = mkIf (cfg.group == "tiny-dfr") {
      tiny-dfr = {};
    };
    
    # Install the package
    environment.systemPackages = [ cfg.package ];
    
    # Systemd service
    systemd.services.tiny-dfr = {
      description = "Tiny Apple silicon touch bar daemon";
      wantedBy = [ "multi-user.target" ];
      after = [
        "systemd-user-sessions.service"
        "getty@tty1.service"
        "plymouth-quit.service"
        "systemd-logind.service"
        "dev-tiny_dfr_display.device"
        "dev-tiny_dfr_backlight.device"
        "dev-tiny_dfr_display_backlight.device"
      ];
      bindsTo = [
        "dev-tiny_dfr_display.device"
        "dev-tiny_dfr_backlight.device"
        "dev-tiny_dfr_display_backlight.device"
      ];
      
      serviceConfig = {
        Type = "simple";
        ExecStart = "${cfg.package}/bin/tiny-dfr";
        Restart = "always";
        RestartSec = "1";
        User = cfg.user;
        Group = cfg.group;
        DynamicUser = false;
        ProtectSystem = "strict";
        ProtectHome = true;
        NoNewPrivileges = true;
        PrivateTmp = true;
        PrivateDevices = false; # Need access to device files
        PrivateNetwork = true;
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectControlGroups = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@resources"
          "~@privileged"
        ];
        ReadWritePaths = [
          "/var/lib/tiny-dfr"
          "/dev/tiny_dfr_display"
          "/dev/tiny_dfr_backlight"
          "/dev/tiny_dfr_display_backlight"
        ];
      };
    };
    
    # Udev rules
    services.udev.extraRules = ''
      # tiny-dfr touchbar device rules
      SUBSYSTEM=="drm", KERNEL=="tiny_dfr_display*", TAG+="systemd", ENV{SYSTEMD_WANTS}="tiny-dfr.service"
      SUBSYSTEM=="backlight", KERNEL=="tiny_dfr_backlight*", TAG+="systemd", ENV{SYSTEMD_WANTS}="tiny-dfr.service"
      SUBSYSTEM=="backlight", KERNEL=="tiny_dfr_display_backlight*", TAG+="systemd", ENV{SYSTEMD_WANTS}="tiny-dfr.service"
      
      # Seat assignment for touchbar
      SUBSYSTEM=="input", KERNEL=="event*", ENV{ID_INPUT_TOUCHPAD}=="1", ENV{ID_INPUT_TOUCHSCREEN}=="1", TAG+="systemd", ENV{SYSTEMD_WANTS}="tiny-dfr.service"
    '';
    
    # Security policies
    security.polkit.enable = true;
    
    # Hardware support
    hardware.opengl.enable = true;
    hardware.opengl.driSupport = true;
    
    # Networking (if needed for any features)
    networking.firewall.allowedTCPPorts = [ ];
    networking.firewall.allowedUDPPorts = [ ];
  };
  
  meta = {
    maintainers = with maintainers; [ ];
    description = "Tiny Apple silicon touch bar daemon";
    longDescription = ''
      tiny-dfr is a minimal dynamic function row daemon for Apple silicon MacBooks.
      It provides a customizable touch bar interface with various modules and helpers.
      
      This module sets up the systemd service, user account, and udev rules
      needed to run tiny-dfr on NixOS.
    '';
  };
} 