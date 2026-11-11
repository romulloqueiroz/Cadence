"""Independent prototype-domain reference for legacy IDs 4/8/9/10/11/12.

Frequency responses evaluate H((z-w)/(1-w*z)) directly. Common gain is derived
from numerator/denominator evaluations at DC, without the Rust expansion.
Nonlinear traces use these derived polynomials and a separate direct-form-I
simulator, with native compression placement and float prototype constants.
This is static numerical evidence, not an observed Kontakt render.
"""
import cmath, math, struct
from pathlib import Path

def f32(x): return struct.unpack('<f',struct.pack('<f',x))[0]
def bits(x): return struct.unpack('<f',struct.pack('<I',x))[0]
def dbits(x): return struct.unpack('<d',struct.pack('<Q',x))[0]
FOUR=[list(map(bits,p)) for p in [
 [0x3e846b75,0x3e046b75,0x3f1ba37a,0xbe003bba,0x3e046b75],
 [0x3eb208f2,0x3e3208f2,0x3f514087,0xbf034979,0x3e3208f2]]]
SIX=[list(map(bits,p)) for p in [
 [0x3e81d5d5,0x3e01d5d5,0x3f1899d8,0xbdd37d69,0x3e01d5d5],
 [0x3e939041,0x3e139041,0x3f2d700b,0xbe820097,0x3e139041],
 [0x3ec145cb,0x3e4145cb,0x3f63295c,0xbf246f26,0x3e4145cb]]]
REJECT=[list(map(dbits,p)) for p in [
 [0x3fd08d6e92da2f77,0x3fc08d6e92da2f77,0x3fe3746f3973960a,0xbfc0077731371601,0x3fc08d6e92da2f77],
 [0x3fd6411e3c874553,0x3fc6411e3c874553,0x3fea2810de924e77,0xbfe0692f1b1993c9,0x3fc6411e3c874553]]]
F4=[[.647,6.294],[.3,.796]];F6=[[.771,7.786],[.572,2.388],[.149,.473]]

def control(cutoff,rate):
    x=f32(cutoff)*f32(6.15)
    curve=sum(x**n/math.factorial(n) for n in range(1,9))/388.67681884765625
    return min(1,(min(1,curve)+f32(.002))*44100/rate*f32(.998004))
def warp(u,factor,high):
    x=u*f32(math.pi/2)*f32(factor)
    if high:
        x*=f32(.9971332)
        cos=lambda t:1-t*t/2+t**4/24-t**6/720
        return -cos(x+.5)/cos(x-.5)
    x*=f32(.9987809)
    sin=lambda t:t-t**3/6+t**5/120-t**7/5040
    return sin(.5-x)/sin(x+.5)

def model(kind,cutoff,res,rate):
    u=control(cutoff,rate);r=1-(1-f32(res))**2
    if kind==4:
        p=bits(0x3e963993);b=bits(0x3e861406)
        return [([b,b,0,p,0],warp(u,.82,False),False),
                ([b,b,0,p,0],warp(u,.82,True)*.99,True)], [1,1],1
    protos=SIX if kind==12 else REJECT if kind==11 else FOUR
    factors=F6 if kind==12 else F4
    sections=[]
    for i,(base,factor) in enumerate(zip(protos,factors)):
        b1,b2,a1,a2,b0=base
        a1*=1+r*f32(factor[0]);a2*=1+r*f32(factor[1])
        high=kind==9 or (kind in [10,11] and i==1)
        w=warp(u,.82 if kind in [9,10,11] else .99,high)
        sections.append(([b0,b1,b2,a1,a2],w,high))
    def dc_parts(s):
        (b0,b1,b2,a1,a2),w,_=s
        norm=1+a1*w-a2*w*w
        return (1-w)**2*(b0+b1+b2)/norm,(1-w)**2*(1-a1-a2)/norm
    if kind in [10,11]:
        level=(1-r)*.2+.8 if kind==11 else (1-r*r)*f32(.35000002)+f32(.65)
        gains=[d/n*level for n,d in map(dc_parts,sections)]
    else:
        parts=list(map(dc_parts,sections))
        level=(1-r*r)*(.55 if kind==12 else f32(.35000002))+(.45 if kind==12 else f32(.65))
        common=sum(d for n,d in parts)/sum(n for n,d in parts)*level
        gains=[common]*len(sections)
    return sections,gains,1 if kind==11 else (1-r*r)*.25+.75

def transfer(kind,cutoff,res,rate,freq):
    z=cmath.exp(-2j*math.pi*freq/rate)
    sections,gains,out=model(kind,cutoff,res,rate)
    result=[]
    for (p,w,high),gain in zip(sections,gains):
        b0,b1,b2,a1,a2=p;q=-z if high else z
        t=(q-w)/(1-w*q)
        h=(b0+b1*t+b2*t*t)/(1-a1*t-a2*t*t)
        if kind==4:
            at1=(1-w)/(1-w)
            h/= (b0+b1*at1+b2*at1*at1)/(1-a1*at1-a2*at1*at1)
        result.append(h*gain)
    return (sum(result) if kind==11 else math.prod(result))*out

def polynomials(p,w,high,gain):
    # Polynomial products express the substitution independently of the
    # production scalar coefficient formulas.
    def mul(a,b):
        y=[0.]*(len(a)+len(b)-1)
        for i,x in enumerate(a):
            for j,v in enumerate(b):y[i+j]+=x*v
        return y
    b0,b1,b2,a1,a2=p
    A=mul([1,-w],[1,-w]);B=mul([-w,1],[1,-w]);C=mul([-w,1],[-w,1])
    numerator=[(b0*a+b1*b+b2*c)*gain for a,b,c in zip(A,B,C)]
    denominator=[a-a1*b-a2*c for a,b,c in zip(A,B,C)]
    if high:numerator[1]*=-1;denominator[1]*=-1
    return [x/denominator[0] for x in numerator],[x/denominator[0] for x in denominator]

def signal(frame,level):return level if frame%31<13 else -level*.375

def trajectory(kind,rate,cutoff,res,level):
    sections,gains,out=model(kind,cutoff,res,rate)
    if kind==4:
        gains=[]
        for p,w,high in sections:
            b0,b1,b2,a1,a2=p
            gains.append((1-a1-a2)/(b0+b1+b2))
    coeff=[polynomials(p,w,high,g) for (p,w,high),g in zip(sections,gains)]
    states=[[[0.,0.,0.,0.] for _ in coeff] for ch in range(2)]
    compress=lambda x,s:max(-1,min(1,x))+s*(x-max(-1,min(1,x)))
    for frame in range(128):
        outputs=[]
        for ch,x in enumerate([signal(frame,level),-signal(frame,level)*.5]):
            original=x
            if kind==9:x=compress(x,.6)
            branches=[]
            for i,((b,a),s) in enumerate(zip(coeff,states[ch])):
                if kind==11:x=original
                y=sum(v*t for v,t in zip(b,[x,s[0],s[1]]))-sum(v*t for v,t in zip(a[1:],[s[2],s[3]]))
                if kind in [8,10] and i==0:y=compress(y,.6)
                if kind==12 and i==1:y=compress(y,.7)
                s[:]=[x,s[0],y,s[2]]
                x=y;branches.append(y)
            outputs.append((sum(branches) if kind==11 else x)*out)
        yield frame,outputs

if __name__=='__main__':
    folder=Path(__file__).parent
    rows=['# fix rate cutoff resonance frequency real imaginary']
    for kind in [4,8,9,10,11,12]:
     for rate in [8000,48000,96000,192000]:
      for cutoff in [0,.4,.8,1]:
       for res in ([0] if kind==4 else [0,.7,1]):
        for fraction in [.001,.03,.2,.45]:
         freq=rate*fraction;h=transfer(kind,cutoff,res,rate,freq)
         rows.append(f'{kind} {rate} {cutoff} {res} {freq:.9g} {h.real:.12g} {h.imag:.12g}')
    folder.joinpath('higher-filter-responses.tsv').write_text('\n'.join(rows)+'\n')
    rows=['# fix rate cutoff resonance level frame left right']
    for kind in [4,8,9,10,11,12]:
     for rate in [8000,48000,96000]:
      for level in [.01,4,40]:
       for frame,y in trajectory(kind,rate,.5,.7,level):
        rows.append(f'{kind} {rate} .5 .7 {level} {frame} {y[0]:.12g} {y[1]:.12g}')
    folder.joinpath('higher-filter-nonlinear-frames.tsv').write_text('\n'.join(rows)+'\n')
