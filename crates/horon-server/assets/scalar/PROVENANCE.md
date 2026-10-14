# Scalar standalone 자산 출처와 무결성 기록

이 폴더는 npm 공식 배포의 바이트를 그대로 보존한 외부 자산입니다. 생산 코드가 아니며
수정하지 않습니다. `src/lib.rs`의 `/docs` 라우트가 이 파일을 `include_str!` 또는
`include_bytes!`로 내장합니다.

## 고정 대상

| 항목 | 값 |
| --- | --- |
| npm 패키지 | `@scalar/api-reference` |
| 버전 | `1.73.1` (registry `dist-tags.latest`, 조회 시점 2026-10-11) |
| registry | https://registry.npmjs.org/@scalar/api-reference |
| tarball | https://registry.npmjs.org/@scalar/api-reference/-/api-reference-1.73.1.tgz |
| standalone 브라우저 배포 경로 | `package/dist/browser/standalone.js` (IIFE, 자체 완결) |
| 패키지 라이선스 | MIT (`package.json`의 `license`, `author`: Scalar) |
| 저장소 | https://github.com/scalar/scalar (`packages/api-reference`) |

## 저장한 파일

### `scalar-standalone.js`

- 크기(바이트): 4431523
- SHA-256: `62c34a0dda38945864011ebaf6e7fa7812a332429120d18922b34d01c4f7ccc8`
- SHA-512 (base64): 계산하지 않음(개별 번들에는 registry integrity가 없음, tarball에만 있음)
- 노출 전역 API: `window.Scalar = { createApiReference }`
  (번들 내부에서 `typeof window==\`object\`&&(window.Scalar={createApiReference:N5n})`로 할당)
- 자체 완결성: 외부 `chunks/`나 상대 경로 `import`/`require` 참조 없음. CSS는 번들 앞부분에
  `<style id="scalar-style">` 주입 형태로 내장되어 있어 별도 `style.css`가 필요하지 않습니다.

### `LICENSE`

- 크기(바이트): 1071
- SHA-256: `380cd0a6ad700e1f821f2a509f0dd9ff835041cee2d43daf5dedc1adb2bcc620`
- 출처: https://raw.githubusercontent.com/scalar/scalar/main/LICENSE
- 내용: 표준 MIT License, `Copyright (c) 2023-present Scalar`
- 참고: npm tarball에는 루트 LICENSE 파일이 포함되어 있지 않습니다(`package.json`의 `license: MIT`로만
  선언). 그래서 공식 저장소의 LICENSE 원문을 같은 폴더에 함께 보존합니다.

## npm tarball 무결성 검증

| 항목 | 값 |
| --- | --- |
| tarball 크기(바이트) | 10510870 |
| registry `dist.integrity` | `sha512-YatOueurjdSAbE3K51L0NkyzdQEpksnjvzCpJTSMPaYd6VWpXMrycDfTBY0myuhszgxk+PcOehExyIXc20yXhw==` |
| 로컬 계산 sha512 (base64) | `YatOueurjdSAbE3K51L0NkyzdQEpksnjvzCpJTSMPaYd6VWpXMrycDfTBY0myuhszgxk+PcOehExyIXc20yXhw==` (일치) |
| registry `dist.shasum` (sha1) | `ba7d7d622b882d5f68865a691be2b407950b875a` |
| 로컬 계산 sha1 | `ba7d7d622b882d5f68865a691be2b407950b875a` (일치) |
| 로컬 계산 tarball sha256 | `424d2f1e55df6c6a485a374bcebbab32c1945b7f2028219b425d79a07b3b15c6` |
| registry 서명(attestations) | https://registry.npmjs.org/-/npm/v1/attestations/@scalar%2fapi-reference@1.73.1 (SLSA provenance v1 제공) |

## 번들에 내장된 제3자 라이선스 표기

`scalar-standalone.js` 안에 아래 배너가 그대로 들어 있습니다. 별도 파일로 분리하지 않고 원문을
그대로 보존합니다.

| 라이브러리 | 라이선스 | 표기 |
| --- | --- | --- |
| `@scalar/api-reference` 1.73.1 | MIT | 상단 배너, https://github.com/scalar/scalar/blob/main/LICENSE |
| tailwindcss 4.3.3 | MIT | CSS 헤더 `/*! tailwindcss v4.3.3 \| MIT License \| https://tailwindcss.com */` |
| tabbable 6.4.0 | MIT | `/*! tabbable 6.4.0 @license MIT ... */` |
| focus-trap 7.8.0 | MIT | `/*! focus-trap 7.8.0 @license MIT ... */` |

## 갱신 방법

1. `curl -sS https://registry.npmjs.org/@scalar/api-reference`로 `dist-tags.latest`를 확인합니다.
2. 해당 버전 tarball을 내려받아 위 표의 방식으로 sha512(base64)·sha1을 registry `dist` 값과 대조합니다.
3. `tar xzf <tarball> package/dist/browser/standalone.js`로 바이트를 그대로 추출해 이 폴더에
   `scalar-standalone.js`로 덮어씁니다.
4. 새 버전의 LICENSE 원문을 https://raw.githubusercontent.com/scalar/scalar/main/LICENSE 에서 받아
   대조하고, 위 표(버전·해시·크기)를 갱신합니다.
5. `window.Scalar.createApiReference` 노출과 자체 완결성(외부 청크 참조 없음)을 다시 확인합니다.

## 검증 증거

- `.omo/evidence/server-openapi-scalar/task-6/` 아래에 다운로드 로그, 해시 대조, 렌더 스크린샷,
  콘솔 오류 캡처를 남깁니다.
