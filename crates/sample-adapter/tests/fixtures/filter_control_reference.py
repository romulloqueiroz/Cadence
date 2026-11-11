"""Independent time-domain SV1 reference from saved static Kontakt evidence.

The timeline selects controls on the 32-frame grid and evaluates each finite
coefficient ramp in closed form from its boundary value, rather than using the
Rust per-sample state machine. Native audio has not been captured. Cold start
uses Cadence's existing effective-control resume policy.
"""
import math, struct
from pathlib import Path

def f32(x): return struct.unpack('<f',struct.pack('<f',x))[0]
def coefficient(cutoff, rate):
    index=(f32(cutoff)*115+20)*5+f32(1381.8816)
    i=int(index)
    base=f32(2**(i/60-20)); successor=f32(2**((i+1)/60-20))
    hz=base+f32(successor-base)*(index-i)
    x=math.pi*hz/rate
    return x+x**3/3+2*x**5/15

def control(frame):
    # Off-grid changes and a transient that returns before the next tick.
    for at,value in reversed([(0,.2),(13,.8),(109,.1),(163,1),(177,.1),(512,.5)]):
        if frame>=at:return value

def audio(frame):
    return .25 if (frame//23)%2==0 else -.125

rows=['# rate modulated frame cutoff g delta remaining left right']
for rate in [8000,44100,48000,96000,192000]:
 for modulated in [0,1]:
    duration=max(1,math.floor(rate*.002/32+.5))*32
    selected=control(0)
    target=coefficient(selected,rate)
    boundary=target; slope=0; ramp_start=0; current=target
    charge=[0.,0.]
    ticks_left=0
    for frame in range(1536):
        if frame and frame%32==0:
            incoming=control(frame)
            if modulated or incoming!=selected:
                selected=incoming
                target=coefficient(selected,rate)
                boundary=current; ramp_start=frame
                slope=(target-boundary)/duration
                if slope*slope<1e-15:
                    boundary=target; slope=0; ticks_left=0
                else: ticks_left=duration//32
            elif ticks_left:
                ticks_left-=1
                if not ticks_left:
                    boundary=target; slope=0; ramp_start=frame
        current=boundary+slope*(frame-ramp_start+1)
        outputs=[]
        for ch,x in enumerate([audio(frame),-audio(frame)*.5]):
            low=(2*charge[ch]+x*current)/(1+current)
            charge[ch]+=current*(x-low)
            outputs.append(low)
        if frame%19==0 or frame%32 in [0,1,30,31] or frame in [13,109,163,177,512,1535]:
            rows.append(f'{rate} {modulated} {frame} {selected:.9g} {current:.12g} {slope:.12g} {ticks_left} {outputs[0]:.12g} {outputs[1]:.12g}')
Path(__file__).with_name('filter-control-responses.tsv').write_text('\n'.join(rows)+'\n')
