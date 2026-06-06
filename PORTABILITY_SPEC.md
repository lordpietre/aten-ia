# Especificación: Compatibilidad con glibc Antiguos

## Versión: 1.0
## Fecha: 2026-06-06
## Estado: DRAFT

---

## 1. Objetivo

Permitir que el binario `aten-ia` se ejecute en sistemas Linux con versiones antiguas de glibc:
- **Ubuntu 20.04 LTS** (Focal): glibc 2.31
- **Ubuntu 22.04 LTS** (Jammy): glibc 2.35
- **Ubuntu 24.04 LTS** (Noble): glibc 2.39
- **Ubuntu 26.04 LTS** (Plucky): glibc 2.41
- **Debian 12** (Bookworm): glibc 2.36
- **Debian 13** (Trixie): glibc 2.38

## 2. Contexto Actual

### 2.1 Estado del Sistema de Desarrollo
- **Sistema**: Ubuntu 26.04 LTS
- **glibc**: 2.43
- **Binario actual**: Dinámico, depende de:
  - `libc.so.6` (glibc)
  - `libstdc++.so.6` (C++ runtime)
  - `libgomp.so.1` (OpenMP)
  - `libgcc_s.so.1` (GCC runtime)
  - `libm.so.6` (math library)

### 2.2 Problema
Los binarios compilados en glibc 2.43 usan símbolos que no existen en glibc 2.31, causando errores como:
```
/lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.38' not found
```

## 3. Requisitos

### 3.1 Funcionales
- **R-F1**: El binario debe ejecutarse en Ubuntu 20.04+ sin modificaciones del sistema
- **R-F2**: El binario debe ejecutarse en Debian 12+ sin modificaciones del sistema
- **R-F3**: El binario debe mantener todas las funcionalidades actuales (LLM, API, RAG)
- **R-F4**: El tamaño del binario no debe exceder 250MB (actual: ~210MB)

### 3.2 No Funcionales
- **R-NF1**: El proceso de build debe ser reproducible en CI/CD
- **R-NF2**: El tiempo de build no debe exceder 45 minutos
- **R-NF3**: Debe mantener compatibilidad con arquitecturas x86_64 y aarch64
- **R-NF4**: Los paquetes .deb deben instalarse sin dependencias externas de glibc

### 3.3 Restricciones
- **R-C1**: No se puede requerir que el usuario instale glibc manualmente
- **R-C2**: No se puede usar Docker en el sistema del usuario final
- **R-C3**: Debe funcionar en sistemas sin acceso a internet (offline-first)

## 4. Análisis de Opciones

### Opción A: Static Linking con musl libc
**Descripción**: Reemplazar glibc con musl libc (static linking completo)

**Ventajas**:
- ✅ Binario 100% portable (sin dependencias de glibc)
- ✅ Tamaño reducido (~150MB vs 210MB)
- ✅ Seguridad mejorada (musl es más seguro que glibc)
- ✅ Funciona en cualquier Linux desde kernel 2.6+

**Desventajas**:
- ❌ Requiere recompilar llama.cpp con musl
- ❌ Posibles problemas de rendimiento (~5-10% más lento)
- ❌ Algunas librerías C++ pueden tener incompatibilidades
- ❌ Requiere toolchain específico (x86_64-unknown-linux-musl)

**Esfuerzo**: Alto (2-3 semanas)
**Riesgo**: Medio

### Opción B: Compilar en Contenedor con glibc Antiguo
**Descripción**: Usar Ubuntu 20.04 container para compilar

**Ventajas**:
- ✅ Garantiza compatibilidad con glibc 2.31+
- ✅ No requiere cambios en el código
- ✅ Mantiene rendimiento óptimo
- ✅ Fácil de implementar en CI/CD

**Desventajas**:
- ❌ Aún depende de glibc dinámicamente
- ❌ Requiere que el usuario tenga glibc 2.31+ (no funciona en CentOS 7)
- ❌ Tamaño del binario similar al actual

**Esfuerzo**: Bajo (1-2 días)
**Riesgo**: Bajo

### Opción C: Static Linking Parcial (Vendoring)
**Descripción**: Link estáticamente libstdc++, libgomp, pero mantener glibc dinámico

**Ventajas**:
- ✅ Reduce dependencias externas
- ✅ Mantiene compatibilidad con glibc
- ✅ Mejor rendimiento que musl

**Desventajas**:
- ❌ Aún depende de glibc (requiere versión mínima)
- ❌ Puede causar conflictos de símbolos
- ❌ Tamaño del binario aumenta

**Esfuerzo**: Medio (1 semana)
**Riesgo**: Medio

### Opción D: Manylinux Container (Estándar Python)
**Descripción**: Usar manylinux2014 container (glibc 2.17)

**Ventajas**:
- ✅ Máxima compatibilidad (glibc 2.17+ = CentOS 7+)
- ✅ Estándar de la industria (usado por Python, Rust)
- ✅ Binarios probados en múltiples distribuciones

**Desventajas**:
- ❌ Requiere adaptar el build system
- ❌ Contenedor más grande (~2GB)
- ❌ Más complejo de mantener

**Esfuerzo**: Medio-Alto (1-2 semanas)
**Riesgo**: Bajo

## 5. Recomendación

**Opción Seleccionada**: **B + C (Híbrido)**

Compilar en contenedor con glibc antiguo (Ubuntu 20.04) + static linking de libstdc++ y libgomp.

**Justificación**:
1. **Compatibilidad garantizada**: glibc 2.31+ cubre Ubuntu 20.04 y Debian 12
2. **Menor esfuerzo**: No requiere cambios en llama.cpp
3. **Rendimiento óptimo**: Mantiene glibc nativo
4. **Reducción de dependencias**: libstdc++ y libgomp estáticos evitan problemas de versión

## 6. Especificación Técnica

### 6.1 Arquitectura de Build

```
┌─────────────────────────────────────────┐
│   Ubuntu 20.04 Container (glibc 2.31)   │
├─────────────────────────────────────────┤
│  1. Instalar dependencias               │
│     - cmake, clang, libgomp-dev         │
│     - Rust toolchain                    │
│                                         │
│  2. Compilar llama.cpp (static)         │
│     - cmake -DBUILD_SHARED_LIBS=OFF     │
│                                         │
│  3. Compilar aten-ia (static parcial)   │
│     - RUSTFLAGS="-C target-feature=     │
│       +crt-static"                      │
│     - Link libstdc++, libgomp estáticos │
│                                         │
│  4. Validar binario                     │
│     - ldd (verificar dependencias)      │
│     - objdump (verificar símbolos)      │
└─────────────────────────────────────────┘
```

### 6.2 Configuración de Cargo

**Archivo**: `.cargo/config.toml`

```toml
[target.x86_64-unknown-linux-gnu]
rustflags = [
  "-C", "link-arg=-Wl,-Bstatic",
  "-C", "link-arg=-lstdc++",
  "-C", "link-arg=-lgomp",
  "-C", "link-arg=-Wl,-Bdynamic",
  "-C", "link-arg=-lc",
  "-C", "link-arg=-lm",
  "-C", "link-arg=-lgcc_s",
]

[target.aarch64-unknown-linux-gnu]
rustflags = [
  "-C", "link-arg=-Wl,-Bstatic",
  "-C", "link-arg=-lstdc++",
  "-C", "link-arg=-lgomp",
  "-C", "link-arg=-Wl,-Bdynamic",
  "-C", "link-arg=-lc",
  "-C", "link-arg=-lm",
  "-C", "link-arg=-lgcc_s",
]
```

### 6.3 Dockerfile para Build

**Archivo**: `docker/Dockerfile.build`

```dockerfile
FROM ubuntu:20.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
    build-essential \
    cmake \
    clang \
    libclang-dev \
    libgomp-dev \
    libssl-dev \
    pkg-config \
    curl \
    git \
    fakeroot \
    && rm -rf /var/lib/apt/lists/*

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

RUN rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu

WORKDIR /build
COPY . .

CMD ["cargo", "build", "--release"]
```

### 6.4 Script de Build

**Archivo**: `scripts/build-portable.sh`

```bash
#!/bin/bash
set -euo pipefail

echo "==> Building aten-ia portable binary..."

docker build -t aten-ia-builder -f docker/Dockerfile.build .

docker run --rm \
  -v "$(pwd)/target:/build/target" \
  -v "$(pwd)/memvid-agent-core:/build/memvid-agent-core" \
  aten-ia-builder \
  bash -c "cd memvid-agent-core && cargo build --release"

echo "==> Validating binary..."
ldd memvid-agent-core/target/release/aten-ia

echo "==> Checking glibc version requirement..."
objdump -T memvid-agent-core/target/release/aten-ia | grep GLIBC | sort -u

echo "✓ Build complete!"
```

### 6.5 Validación de Compatibilidad

**Archivo**: `scripts/validate-compat.sh`

```bash
#!/bin/bash
set -euo pipefail

BINARY="memvid-agent-core/target/release/aten-ia"

echo "==> Validating portable binary: $BINARY"

# Verificar que existe
if [ ! -f "$BINARY" ]; then
  echo "✗ Binary not found: $BINARY"
  exit 1
fi

# Verificar dependencias dinámicas
echo "==> Dynamic dependencies:"
ldd "$BINARY" | grep -E "libc\.so|libm\.so|libgcc_s\.so" || true

# Verificar que NO depende de libstdc++ ni libgomp dinámicamente
if ldd "$BINARY" | grep -q "libstdc++\.so"; then
  echo "✗ Binary still depends on libstdc++.so dynamically"
  exit 1
fi

if ldd "$BINARY" | grep -q "libgomp\.so"; then
  echo "✗ Binary still depends on libgomp.so dynamically"
  exit 1
fi

# Verificar versión máxima de glibc requerida
MAX_GLIBC=$(objdump -T "$BINARY" | grep GLIBC | sed 's/.*GLIBC_//' | sort -V | tail -1)
echo "==> Maximum glibc version required: GLIBC_$MAX_GLIBC"

# Comparar con 2.31 (Ubuntu 20.04)
if [ "$(printf '%s\n' "2.31" "$MAX_GLIBC" | sort -V | tail -1)" != "2.31" ]; then
  echo "✗ Binary requires glibc newer than 2.31 (Ubuntu 20.04)"
  exit 1
fi

echo "✓ Binary is compatible with Ubuntu 20.04+ (glibc 2.31+)"
echo "✓ Binary is compatible with Ubuntu 22.04+ (glibc 2.35+)"
echo "✓ Binary is compatible with Ubuntu 24.04+ (glibc 2.39+)"
echo "✓ Binary is compatible with Ubuntu 26.04+ (glibc 2.41+)"
echo "✓ Binary is compatible with Debian 12+ (glibc 2.36+)"
echo "✓ Binary is compatible with Debian 13+ (glibc 2.38+)"
```

## 7. Plan de Implementación

### Fase 1: Preparación (Día 1-2)
- [ ] Crear estructura de directorios (`docker/`, `scripts/`)
- [ ] Escribir Dockerfile de build
- [ ] Configurar `.cargo/config.toml` para static linking
- [ ] Crear script de build automatizado

### Fase 2: Implementación (Día 3-5)
- [ ] Adaptar `build.rs` para detectar entorno de contenedor
- [ ] Modificar `release.yml` para usar contenedor Ubuntu 20.04
- [ ] Implementar static linking de libstdc++ y libgomp
- [ ] Probar build local con Docker

### Fase 3: Validación (Día 6-7)
- [ ] Ejecutar tests en Ubuntu 20.04 (VM o contenedor)
- [ ] Ejecutar tests en Debian 12 (VM o contenedor)
- [ ] Validar que todas las funcionalidades operan correctamente
- [ ] Verificar tamaño del binario (<250MB)

### Fase 4: Integración CI/CD (Día 8-10)
- [ ] Actualizar `.github/workflows/release.yml`
- [ ] Configurar GitHub Actions para usar contenedor
- [ ] Agregar job de validación de compatibilidad
- [ ] Documentar proceso de release

### Fase 5: Documentación y Release (Día 11-12)
- [ ] Actualizar README.md con requisitos de sistema
- [ ] Documentar proceso de build para contribuidores
- [ ] Crear release candidate y probar en múltiples distros
- [ ] Publicar release final

## 8. Criterios de Aceptación

### 8.1 Criterios Funcionales
- [ ] El binario se ejecuta en Ubuntu 20.04 sin errores
- [ ] El binario se ejecuta en Debian 12 sin errores
- [ ] Todas las funcionalidades operan correctamente (LLM, API, RAG)
- [ ] El binario es identificable como `aten-ia` en `ps`
- [ ] El shutdown graceful funciona correctamente

### 8.2 Criterios Técnicos
- [ ] `ldd` muestra solo dependencias de libc, libm, libgcc_s
- [ ] `objdump` muestra GLIBC_2.31 o inferior como versión máxima
- [ ] El tamaño del binario es <250MB
- [ ] El build se completa en <45 minutos en CI

### 8.3 Criterios de Calidad
- [ ] Todos los tests pasan (`cargo test -- --test-threads=1`)
- [ ] `cargo clippy --lib` no tiene errores
- [ ] `cargo fmt --check` pasa sin cambios
- [ ] Documentación actualizada

## 9. Riesgos y Mitigación

| Riesgo | Probabilidad | Impacto | Mitigación |
|--------|--------------|---------|------------|
| Static linking causa conflictos de símbolos | Media | Alto | Probar con múltiples versiones de GCC |
| Binario no funciona en aarch64 | Baja | Alto | Validar en Raspberry Pi 4 (ARM64) |
| Tiempo de build excede 45 min | Media | Medio | Usar caché de Docker y Rust |
| Problemas de rendimiento con static linking | Baja | Medio | Benchmark comparativo antes/después |

## 10. Métricas de Éxito

1. **Compatibilidad**: 100% de éxito en Ubuntu 20.04/22.04/24.04/26.04, Debian 12/13
2. **Rendimiento**: <5% degradación vs binario dinámico
3. **Tamaño**: <250MB (objetivo: 200MB)
4. **Build time**: <45 minutos en GitHub Actions

## 11. Referencias

- [Rust Static Linking Guide](https://doc.rust-lang.org/reference/linkage.html)
- [Manylinux Specification](https://github.com/pypa/manylinux)
- [Ubuntu 20.04 Release Notes](https://wiki.ubuntu.com/FocalFossa/ReleaseNotes)
- [Debian 12 Release Notes](https://www.debian.org/releases/bookworm/)

---

**Aprobado por**: _Pendiente_
**Fecha de aprobación**: _Pendiente_
