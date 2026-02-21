function readStdin(){const c=[];const b=new Uint8Array(4096);while(true){const n=Javy.IO.readSync(0,b);if(n===0)break;c.push(b.slice(0,n))}const t=c.reduce((s,x)=>s+x.length,0);const r=new Uint8Array(t);let o=0;for(const x of c){r.set(x,o);o+=x.length}return new TextDecoder().decode(r)}
const params = JSON.parse(readStdin());
const data = params.data || "";
const algorithm = (params.algorithm || "fnv1a").toLowerCase();
function fnv1a32(str){let h=0x811c9dc5;for(let i=0;i<str.length;i++){h^=str.charCodeAt(i);h=Math.imul(h,0x01000193)}return(h>>>0).toString(16).padStart(8,"0")}
function djb2(str){let h=5381;for(let i=0;i<str.length;i++){h=((h<<5)+h+str.charCodeAt(i))&0xffffffff}return(h>>>0).toString(16).padStart(8,"0")}
function crc32(str){let c=0xffffffff;for(let i=0;i<str.length;i++){c^=str.charCodeAt(i);for(let j=0;j<8;j++){c=(c>>>1)^(c&1?0xedb88320:0)}}return((c^0xffffffff)>>>0).toString(16).padStart(8,"0")}
let result;
try {
  switch(algorithm){
    case "fnv1a": result={hash:fnv1a32(data),algorithm:"fnv1a-32",length:data.length};break;
    case "djb2": result={hash:djb2(data),algorithm:"djb2",length:data.length};break;
    case "crc32": result={hash:crc32(data),algorithm:"crc32",length:data.length};break;
    case "all": result={fnv1a:fnv1a32(data),djb2:djb2(data),crc32:crc32(data),length:data.length};break;
    default: result={error:"Unknown algorithm: "+algorithm};
  }
} catch(e){result={error:e.message}}
Javy.IO.writeSync(1, new TextEncoder().encode(JSON.stringify(result)));
