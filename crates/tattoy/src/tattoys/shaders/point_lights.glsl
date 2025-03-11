// Original code from: https://www.shadertoy.com/view/MsScRc

#define pixelWidth 1.0/iResolution.x
#define offset_a pixelWidth * 0.5
#define offset_b pixelWidth * 0.8

struct Light {
    vec3 color;
    vec2 pos;
    float min;
    float max;
    float brightness;
};
 
//Line is used to define lines and boxes
struct Line {
    vec2 start;
    vec2 end;
};

//Define box to obstruct light   
Line box = Line(vec2(0.7, 0.5), vec2(0.8, 0.6));

Light light0 = Light(vec3(1.0, 1.0, 1.0), vec2(0.5, 0.7), 0.0, 1.5, 0.5);


float hyperstep(float min, float max, float x) {    	            
    if (x < min) {
        return 1.0;
    }
    else if (x > max) {
        return 0.0;
    }
    else {
        //linear interpolation of x between min and max
        float value= (x - min) / (max - min);

        //hyperbolic function: 100/99 * (9x + 1)^2 - 1/99
        return (100./99.) / ((9. * value + 1.) * (9. * value + 1.)) - (1./99.);
    }
}



vec2 intersectPoint(Line line_0, Line line_1) {
    float slope_0, slope_1, x, y;
    
    if (line_0.start.x == line_0.end.x) {
        
        //slope_0 is infinite
        slope_1= (line_1.start.y - line_1.end.y) / (line_1.start.x - line_1.end.x);
        
        x= line_0.start.x;
        y= slope_1 * x + line_1.start.y;
        
    }
    else if (line_1.start.x == line_1.end.x) {
        
        //slope_1 is infinite
        slope_0= (line_0.start.y - line_0.end.y) / (line_0.start.x - line_0.end.x);
        
        x= line_1.start.x;
        y= slope_0 * (x - line_0.start.x) + line_0.start.y;
        
    }
    else {
    
        slope_0= (line_0.start.y - line_0.end.y) / (line_0.start.x - line_0.end.x);
        slope_1= (line_1.start.y - line_1.end.y) / (line_1.start.x - line_1.end.x);

        if (slope_0 != slope_1) {
			
            //calculate y-intercept of line_1 based on line_0.start
            float b= slope_1 * (line_0.start.x - line_1.start.x) + line_1.start.y;
            
            x= (b - line_0.start.y) / (slope_0 - slope_1);
            y= slope_0 * x + line_0.start.y;
            x= x + line_0.start.x;
            

        }
        //lines are parallel
        else return vec2(-1.0);
    }
    
    
    return vec2(x, y);
}

bool inside(Line box, vec2 point) {
    vec2 minValues = vec2( min(box.start.x, box.end.x), min(box.start.y, box.end.y) );
    vec2 maxValues = vec2( max(box.start.x, box.end.x), max(box.start.y, box.end.y) );
    
    if (point.x < minValues.x) return false;
    if (point.x > maxValues.x  ) return false;
    if (point.y < minValues.y) return false;
    if (point.y > maxValues.y  ) return false;
    return true;
}

bool intersects(Line a, Line b) {
    vec2 point = intersectPoint(a, b);
    return inside(a, point) && inside(b, point);
}

vec3 calculateLighting(vec2 pixel, Light light) {
    Line LoS = Line(pixel, light.pos);
    
    if (
				intersects(LoS, Line(box.start, vec2(box.end.x, box.start.y))) ||
        intersects(LoS, Line(box.start, vec2(box.start.x, box.end.y))) ||
        intersects(LoS, Line(box.end, vec2(box.start.x, box.end.y)))   ||
        intersects(LoS, Line(box.end, vec2(box.end.x, box.start.y)))
		) {    
        return vec3(0.0);
    } else {         
        return hyperstep(
					light.min,
					light.max,
					distance(pixel, light.pos)
				) * light.brightness * light.color;
    }
}

vec3 multisample(vec2 pixel) {
    vec2 points[4];
    
    points[0] = pixel + vec2(offset_a, offset_b);
    points[1] = pixel + vec2(-offset_a, -offset_b);
    points[2] = pixel + vec2(offset_b, -offset_a);
    points[3] = pixel + vec2(-offset_b, -offset_a);
    
    vec3 color = vec3(0.0);
    
    for (int i= 0; i < 4; i++) {
        color+= calculateLighting(points[i], light0);
        // color+= calculateLighting(points[i], light1);
    }
    
    return color / 4.0;
}

void mainImage( out vec4 fragColor, in vec2 fragCoord ) {
    
    vec2 pixel= fragCoord / iResolution.y;
    
    light0.pos= iMouse.xy / iResolution.y;
    
    // light1.pos.y= sin(iTime) / 4.0 + 0.5;
    
    //background color
    vec3 color= vec3(0.0, 0.0, 0.0);
      
    //anti-aliasing
    color+= multisample(pixel);
    
    //without anti-aliasing
    //color+= calculateLighting(pixel, light0);
    //color+= calculateLighting(pixel, light1);
    
    if (inside(box, pixel)) {
        //box color
        color = vec3(0.25, 0.4, 0.0);
    }
    
    fragColor= vec4(color, 1.0);
}
