{
  # Matrix/Synapse
  services.postgresql = {
    enable = true;
    initialScript = pkgs.writeText "synapse-init.sql" ''
      CREATE ROLE "matrix-synapse" WITH LOGIN PASSWORD '${builtins.readFile ../secrets/matrix-sql-pw}';
      CREATE DATABASE "matrix-synapse" WITH OWNER "matrix-synapse"
        TEMPLATE template0
        LC_COLLATE = "C"
        LC_CTYPE = "C";
    '';
  };

  services.matrix-synapse = {
    enable = true;
    enable_metrics = true;
    enable_registration = false;
    allow_guest_access = false;
    registration_shared_secret = builtins.readFile ../secrets/matrix-registration.key;

    dynamic_thumbnails = true;
    listeners = [
      {
        bind_address = "89.101.222.210";
        port = 8448;
        resources = [
          {
            compress = false;
            names = [ "client" "webclient" "federation" ];
          }
        ];
        tls = false;
        type = "http";
        x_forwarded = false;
      }
    ];
    public_baseurl = "https://matrix.brage.info/";
    server_name = "brage.info";

    logConfig = ''
      version: 1

      # In systemd's journal, loglevel is implicitly stored, so let's omit it
      # from the message text.
      formatters:
          journal_fmt:
              format: '%(name)s: [%(request)s] %(message)s'

      filters:
          context:
              (): synapse.util.logcontext.LoggingContextFilter
              request: ""

      handlers:
          journal:
              class: systemd.journal.JournalHandler
              formatter: journal_fmt
              filters: [context]
              SYSLOG_IDENTIFIER: synapse

      root:
          level: WARNING
          handlers: [journal]

      disable_existing_loggers: False
    '';
  };
}
