# 루팅 후 사용자모임 동봉 자료

사용자 요청(2026-10-08)에 따라 아래 파일을 저장소에 포함합니다. 카페 로그인 쿠키·임시 다운로드 주소는 포함하지 않습니다. 실행 코드를 바꾸거나 ZIP을 재압축하지 않았습니다. 출처 기록은 저작권 표기를 대체하지 않으며 각 원본의 포함된 저작권/라이선스 표기도 유지합니다.

공통 출처: [앨리자, Xperia 루팅 후 기본 세팅 정리](https://cafe.naver.com/x1smart/617249), 게시 2026-09-23, 본문 갱신일 2026-10-01, 취득 2026-10-08.

| 파일 | 원본/취득 경로 | 바이트 | SHA-256 |
|---|---|---:|---|
| `Meta-Overlayfsx_v1.3.4_13400.zip` | 카페 첨부; [원 프로젝트](https://github.com/RipperHybrid/Meta-Overlayfsx) | 569836 | `043e01d944ab40327b64aeba1e8a88c8c616c36f1f6ba7c69216cf418a4832ea` |
| `AshReXcue_Bootloop_Protector_9.9_KO_SonyUserCommunity.zip` | 카페 첨부 한국어 빌드; [원 프로젝트](https://github.com/RipperHybrid/AshLooper) | 1034305 | `8d6445c90ecfa237b8c7a09a06f8169bad9035a22858e99fc0741e877668f408` |
| `SonyUserCommunity_PlayStoreFix_v3.4_Magisk-KernelSU.zip` | 본문 Google Drive 링크; 제작 표기 소니사용자모임, DoubleHack·CITRA·T3SL4 기반 | 3012793 | `3a1caa209914221350baac783d6da645eb60ec5e64f680fbc745e912b0ed6c34` |
| `HMA-OSS_SonyUserCommunity_2026-10-01.json` | 앞선 세션에서 확보한 카페 첨부 프리셋의 로컬 사본 | 122350 | `99f730a53ba3474b0581bb28622844ece43dd4cdeb479c6ce13b53d6af897dfb` |

ZIP 세 개와 프리셋은 `include_bytes!`로 프로그램에 포함하고 고정 해시를 검사합니다. ZIP은 module.prop·경로/중복/링크·읽기/CRC·압축 해제 크기도 검사한 뒤 캐시에 복사합니다. 원본 자동 갱신은 하지 않습니다. 파일을 교체할 때는 출처/버전/해시·코드 핀·검증 기록을 같은 커밋에서 갱신해야 합니다.

ReSukiSU/Magisk와 공개 GitHub 모듈은 이 폴더에 넣지 않습니다. 선택한 버전 또는 최신 stable의 자산을 실행 시 취득해 검증합니다. HMA 프리셋은 기기 설정을 자동 변경하지 않고 사용자가 선택한 PC 폴더로 내보냅니다. 은행 앱 scope는 사용자가 보강하고 결과를 직접 확인해야 합니다.
