{...}: {
  services.nginx.appendHttpConfig = ''
    include ${./nginx/mime_types.conf};
  '';
}
