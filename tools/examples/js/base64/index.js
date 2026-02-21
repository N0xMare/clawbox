function readStdin(){const c=[];const b=new Uint8Array(4096);while(true){const n=Javy.IO.readSync(0,b);if(n===0)break;c.push(b.slice(0,n))}const t=c.reduce((s,x)=>s+x.length,0);const r=new Uint8Array(t);let o=0;for(const x of c){r.set(x,o);o+=x.length}return new TextDecoder().decode(r)}
const params = JSON.parse(readStdin());
const action = params.action || "encode";
const data = params.data || "";
const chars="ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
function b64encode(str){const b=new TextEncoder().encode(str);let r="";for(let i=0;i<b.length;i+=3){const a=b[i],c=b[i+1]||0,d=b[i+2]||0;r+=chars[a>>2];r+=chars[((a&3)<<4)|(c>>4)];r+=(i+1<b.length)?chars[((c&15)<<2)|(d>>6)]:"=";r+=(i+2<b.length)?chars[d&63]:"="}return r}
function b64decode(str){const lk={};for(let i=0;i<chars.length;i++)lk[chars[i]]=i;const cl=str.replace(/[=\s]/g,"");const b=[];for(let i=0;i<cl.length;i+=4){const a=lk[cl[i]]||0,c=lk[cl[i+1]]||0,d=lk[cl[i+2]]||0,e=lk[cl[i+3]]||0;b.push((a<<2)|(c>>4));if(cl[i+2])b.push(((c&15)<<4)|(d>>2));if(cl[i+3])b.push(((d&3)<<6)|e)}return new TextDecoder().decode(new Uint8Array(b))}
let result;
try{if(action==="encode")result={encoded:b64encode(data),length:data.length};else if(action==="decode")result={decoded:b64decode(data),original_length:data.length};else result={error:"Unknown action: "+action}}catch(e){result={error:e.message}}
Javy.IO.writeSync(1, new TextEncoder().encode(JSON.stringify(result)));
