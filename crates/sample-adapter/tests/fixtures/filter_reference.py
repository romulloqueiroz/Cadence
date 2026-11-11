"""Independent complex transfer reference, based on the saved native equations.

Legacy: substitute t=(z-w)/(1-w*z) into the initialized prototype transfer,
normalize at DC, then substitute -z for high-pass. No production coefficient
expansion or sample processor is imported. SV1: solve its integrator transfer.
Numbers are f64 except recovered float constants/table entries. This is static
reference evidence, not a capture from a running Kontakt instance.
"""
import cmath, math, struct
from pathlib import Path

def f32(x): return struct.unpack('<f', struct.pack('<f', x))[0]
def bits(x): return struct.unpack('<f', struct.pack('<I', x))[0]

def transfer(kind, cutoff, resonance, rate, frequency):
    z=cmath.exp(-2j*math.pi*frequency/rate)
    if kind>=50:
        index=(cutoff*115+20)*5+f32(1381.8816)
        i=int(index)
        lo=f32(2**(i/60-20)); hi=f32(2**((i+1)/60-20))
        hz=lo+f32(hi-lo)*(index-i)
        x=math.pi*hz/rate
        g=x*(1+x*x/3+2*x**4/15)
        low=g*(1+z)/(1+g+(g-1)*z)
        return 1-low if kind==51 else low
    high=kind in (3,7); two=kind in (6,7)
    x=cutoff*f32(6.15)
    curve=sum(x**n/math.factorial(n) for n in range(1,9))/388.67681884765625
    control=min(1,(min(1,curve)+f32(.002))*44100/rate*f32(.998004))
    if high:
        x=control*f32(math.pi/2)*f32(.92 if two else .82)*f32(.9971332)
        cos=lambda t:1-t*t/2+t**4/24-t**6/720
        w=-cos(x+.5)/cos(x-.5)
    else:
        x=control*f32(math.pi/2)*f32(.99)*f32(.9987809)
        sin=lambda t:t-t**3/6+t**5/120-t**7/5040
        w=sin(.5-x)/sin(x+.5)
    def prototype(v):
        if two:
            r=1-(1-resonance)**2
            return (bits(0x3e139041)+bits(0x3e939041)*v+bits(0x3e139041)*v*v)/(1-bits(0x3f2d700b)*v-bits(0xbe820097)*(1+r*f32(2.687))*v*v)
        return bits(0x3e861406)*(1+v)/(1-bits(0x3e963993)*v)
    def warped(q): return prototype((q-w)/(1-w*q))
    level=1 if not two else (1-(1-(1-resonance)**2))*.55+.45
    return warped(-z if high else z)/warped(1)*level

rows=['# fix rate cutoff resonance frequency real imaginary']
for kind in [2,3,6,7,50,51]:
 for rate in [8000,44100,48000,96000]:
  for cutoff in [0,.4,.8,1]:
   for res in ([0,.7,1] if kind in [6,7] else [0]):
    for fraction in [.001,.03,.2,.45]:
     freq=rate*fraction
     h=transfer(kind,cutoff,res,rate,freq)
     rows.append(f'{kind} {rate} {cutoff} {res} {freq:.9g} {h.real:.12g} {h.imag:.12g}')
Path(__file__).with_name('filter-responses.tsv').write_text('\n'.join(rows)+'\n')
