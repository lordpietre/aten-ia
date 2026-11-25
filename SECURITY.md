# Seguridad

## Versiones soportadas

Se aplican parches de seguridad a la última versión publicada (`0.3.x`).

| Versión | Soporte |
|---|---|
| 0.3.x | ✅ |
| <= 0.2.x | ❌ |

## Cómo reportar una vulnerabilidad

**No abras una issue pública** para vulnerabilidades sin corregir.

Envía un correo a **dinamicas.electronicas@gmail.com** con:

- Versión de aten-ia y sistema operativo.
- Descripción del problema y su impacto.
- Pasos de reproducción o prueba de concepto.
- Si lo deseas, una propuesta de corrección.

Recibirás acuse de recibo en un plazo orientativo de 72 horas. El proyecto es
mantenido por Dinámicas Electrónicas; no hay programa de recompensas.

## Modelo de amenazas (resumen)

- aten-ia está pensado para ejecutarse **en local**, bajo el usuario que lo
  lanza. No es un servicio multiusuario.
- La API HTTP no usa TLS. No la expongas a Internet sin un proxy inverso con
  TLS. El token viaja en la cabecera `Authorization: Bearer`.
- Los modelos GGUF y las bibliotecas precompiladas son código/datos nativos:
  descárgalos solo de fuentes confiables y verifica sus checksums.
- Los documentos, feeds y `.mv2` se consideran **entradas no confiables**: no
  ejecutes aten-ia sobre datos de terceros en un entorno privilegiado.
- `config.json`, `.env` y la configuración de OpenCode pueden contener secretos.
  Mantenlos con permisos `0600` y fuera del control de versiones.

## Buenas prácticas

1. Mantén `api.host` en `127.0.0.1` salvo que necesites acceso remoto.
2. Rota el token de la API si sospechas que se ha filtrado.
3. Ejecuta `cargo audit` antes de compilar releases.
4. Verifica el SHA-256 de los binarios y modelos descargados.
5. No ejecutes el binario en directorios con `.env` no confiables.
