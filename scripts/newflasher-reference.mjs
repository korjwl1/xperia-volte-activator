// Generates an OFFLINE C harness from the pinned upstream functions, without USB code.
// node scripts/newflasher-reference.mjs <upstream newflasher.c> <new output directory>
import fs from "node:fs";
import path from "node:path";
import crypto from "node:crypto";
const [sourcePath, destination] = process.argv.slice(2);
if (!sourcePath || !destination || !path.isAbsolute(destination)) throw new Error("Provide source and absolute output directory");
const bytes = fs.readFileSync(sourcePath);
const sha = crypto.createHash("sha256").update(bytes).digest("hex");
if (sha !== "79804a4df5a35e507a96df827083ca3eac3f70d068223ca2a70246f4ad2f9e17") throw new Error("Upstream source hash mismatch");
const source = bytes.toString("utf8").replaceAll("\r\n", "\n");
function block(signature) {
  const start = source.indexOf(signature);
  if (start < 0) throw new Error("Missing source function: " + signature);
  const end = source.indexOf("\n}\n", start);
  if (end < 0) throw new Error("Missing function end");
  return source.slice(start, end + 3);
}
const helpers = ["static char *basenamee", "unsigned int file_size", "static int parseoct", "static int is_end_of_archive", "static FILE *create_file", "static int verify_checksum"].map(block).join("\n");
const harness = `${source.slice(0, source.indexOf("*/") + 2)}
/* Offline harness; see src-tauri/src/flasher/NOTICE.md for scope and adaptations. */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdbool.h>
#include <stdint.h>
#include <sys/stat.h>
#ifdef _WIN32
#include <direct.h>
#define fopen64 fopen
#define fseeko64 _fseeki64
#else
#include <unistd.h>
#include <limits.h>
#define fopen64 fopen
#define fseeko64 fseeko
#endif
typedef int HANDLE;
#define EP_IN 0
#define EP_OUT 1
#define USB_TIMEOUT 120000
#define BUFF_MAX 0x1000000
static char tmp[4096], tmp_reply[BUFF_MAX], queued[64];
static unsigned long get_reply_len;
static bool is_2021_device, keep_userdata=false, file_found_in_updatexml=false;
static char current_slot[2]="b";
static unsigned long pending=0;
static void display_buffer_hex_ascii(char*a,char*b,unsigned int c) {(void)a;(void)b;(void)c;}
static void check_in_updatexml(char*a,char*b) {(void)a;(void)b; fprintf(stderr,"Unexpected NOERASE call"); exit(2);}
static unsigned long transfer_bulk_async(HANDLE dev,int ep,char *bytes,unsigned long size,int timeout,int exact) {
  (void)dev;(void)timeout;(void)exact;
  if(ep==EP_IN) { unsigned long n=(unsigned long)strlen(queued); if(n>size) exit(3); memcpy(bytes,queued,n); queued[0]=0; return n; }
  printf("TRACE "); for(unsigned long i=0;i<size;i++) printf("%02x",(unsigned char)bytes[i]); printf("\\n");
  if(pending) { if(size>pending) exit(4); pending-=size; if(!pending) strcpy(queued,"OKAY"); return size; }
  if((size>9 && memcmp(bytes,"download:",9)==0) || (size>10 && memcmp(bytes,"signature:",10)==0)) {
    char cmd[128]; if(size>=sizeof(cmd)) exit(5); memcpy(cmd,bytes,size); cmd[size]=0;
    char *length=strchr(cmd,':')+1; pending=strtoul(length,NULL,16); snprintf(queued,sizeof(queued),"DATA%08lx",pending);
  } else if(size>16 && memcmp(bytes,"getvar:has-slot:",16)==0) { strcpy(queued,"OKAYyes"); }
  else { strcpy(queued,"OKAY"); }
  return size;
}
${helpers}
${block("static bool get_reply")}
${block("static int process_sins")}
int main(int argc,char **argv) {
  if(argc!=3) return 2;
  is_2021_device=strcmp(argv[2],"modern")==0;
  FILE *input=fopen(argv[1],"rb"); if(!input) return 3;
#ifdef _WIN32
  char *root=_getcwd(NULL,0);
#else
  char root[PATH_MAX]; if(!getcwd(root,sizeof(root))) return 4;
#endif
  int result=process_sins(0,input,argv[1],root,"flash_session","flash");
  fclose(input); return result?0:1;
}
`;
fs.mkdirSync(destination, { recursive: true });
fs.mkdirSync(path.join(destination,"flash_session"), { recursive: true });
fs.writeFileSync(path.join(destination,"reference.c"), harness, { flag: "wx" });
// Independent minimal USTAR fixture; never Sony firmware or a valid CMS signature.
const records = [["boot.cms", "TEST-CMS"], ["boot.000", "first"], ["boot.001", "second"]];
const tar = [];
for (const [name, text] of records) {
  const body = Buffer.from(text), header = Buffer.alloc(512);
  header.write(name); header.write("0000644\0",100); header.write("0000000\0",108); header.write("0000000\0",116);
  header.write(body.length.toString(8).padStart(11,"0")+"\0",124); header.write("00000000000\0",136);
  header.fill(32,148,156); header[156]=48; header.write("ustar\0",257); header.write("00",263);
  const sum=header.reduce((a,b)=>a+b,0); header.write(sum.toString(8).padStart(6,"0")+"\0 ",148);
  tar.push(header,body,Buffer.alloc((512-body.length%512)%512));
}
tar.push(Buffer.alloc(1024));
fs.writeFileSync(path.join(destination,"boot_X-FLASH-ALL-test.sin"),Buffer.concat(tar),{flag:"wx"});
console.log(JSON.stringify({ upstreamSha256: sha, harness: "reference.c", fixture: "boot_X-FLASH-ALL-test.sin", hardwareAccess: false }));
