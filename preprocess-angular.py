#!/usr/bin/env python3
"""
Pre-process Angular templates to preserve control flow structures.
Converts @if/@for/@switch to temporary XML elements, parses, then restores.
"""

import re
import sys
import subprocess
import tempfile
import os

def preprocess_angular_to_xml(content):
    """Convert Angular control flow to temporary XML elements."""
    result = content
    
    # Convert @if with optional variable assignment
    result = re.sub(r'@if\s*\(\s*([^;]+?)(?:\s*;\s*as\s+(\w+))?\s*\)\s*\{', 
                   lambda m: f'<ng-if condition="{m.group(1).strip()}"' + 
                           (f' variable="{m.group(2)}"' if m.group(2) else '') + '>', result)
    
    # Convert @else if
    result = re.sub(r'\}\s*@else\s+if\s*\(\s*([^)]+)\s*\)\s*\{', 
                   r'</ng-if><ng-else-if condition="\1">', result)
    
    # Convert @else  
    result = re.sub(r'\}\s*@else\s*\{', r'</ng-if><ng-else>', result)
    
    # Convert @for
    result = re.sub(r'@for\s*\(\s*(\w+)\s+of\s+([^;]+)\s*;\s*track\s+([^;)]+)(?:\s*;\s*([^)]+))?\s*\)\s*\{',
                   lambda m: f'<ng-for item="{m.group(1)}" collection="{m.group(2).strip()}" track="{m.group(3).strip()}"' +
                           (f' variables="{m.group(4).strip()}"' if m.group(4) else '') + '>', result)
    
    # Convert @empty
    result = re.sub(r'\}\s*@empty\s*\{', r'</ng-for><ng-empty>', result)
    
    # Convert @switch
    result = re.sub(r'@switch\s*\(\s*([^)]+)\s*\)\s*\{', r'<ng-switch expression="\1">', result)
    
    # Convert @case
    result = re.sub(r'@case\s*\(\s*([^)]+)\s*\)\s*\{', r'<ng-case value="\1">', result)
    
    # Convert @default
    result = re.sub(r'@default\s*\{', r'<ng-default>', result)
    
    # Convert remaining standalone closing braces to appropriate closing tags
    # This is simplified - a production version would need more sophisticated tracking
    result = re.sub(r'\}(?=\s*(?:$|@|<|\}))', '</ng-block>', result)
    
    return result

def postprocess_xml_to_angular(content):
    """Convert XML elements back to Angular control flow syntax."""
    
    # Process ng-if elements
    content = re.sub(r'^\s*ng-if \[condition\]: ([^\n]+)$', 
                    r'@if (\1) {', content, flags=re.MULTILINE)
    
    content = re.sub(r'^\s*ng-if \[condition\]: ([^\n]+)\n\s*\[variable\]: ([^\n]+)$', 
                    r'@if (\1; as \2) {', content, flags=re.MULTILINE)
    
    # Process ng-else-if elements  
    content = re.sub(r'^\s*ng-else-if \[condition\]: ([^\n]+)$', 
                    r'} @else if (\1) {', content, flags=re.MULTILINE)
    
    # Process ng-else elements
    content = re.sub(r'^\s*ng-else$', r'} @else {', content, flags=re.MULTILINE)
    
    # Process ng-for elements
    def replace_for(match):
        lines = match.group(0).split('\n')
        attrs = {}
        for line in lines:
            if '[' in line and ']:' in line:
                key = line.strip().split('[')[1].split(']:')[0]
                value = line.strip().split(']: ')[1]
                attrs[key] = value
        
        result = f"@for ({attrs.get('item', '')} of {attrs.get('collection', '')}; track {attrs.get('track', '')}"
        if 'variables' in attrs:
            result += f"; {attrs['variables']}"
        result += ") {"
        return result
    
    content = re.sub(r'^\s*ng-for.*?(?=^\s*[^ \[]|\Z)', replace_for, content, flags=re.MULTILINE | re.DOTALL)
    
    # Process ng-empty elements
    content = re.sub(r'^\s*ng-empty$', r'} @empty {', content, flags=re.MULTILINE)
    
    # Process ng-switch elements
    content = re.sub(r'^\s*ng-switch \[expression\]: ([^\n]+)$', 
                    r'@switch (\1) {', content, flags=re.MULTILINE)
    
    # Process ng-case elements
    content = re.sub(r'^\s*ng-case \[value\]: ([^\n]+)$', 
                    r'@case (\1) {', content, flags=re.MULTILINE)
    
    # Process ng-default elements
    content = re.sub(r'^\s*ng-default$', r'@default {', content, flags=re.MULTILINE)
    
    # Add closing braces for better formatting
    content = re.sub(r'^\s*ng-block$', r'}', content, flags=re.MULTILINE)
    
    return content

def main():
    if len(sys.argv) < 2:
        print("Usage: python preprocess-angular.py <angular-template-file>")
        print("Converts Angular control flow to preservable format, parses, then restores.")
        sys.exit(1)
    
    input_file = sys.argv[1]
    
    try:
        # Read the original file
        with open(input_file, 'r', encoding='utf-8') as f:
            original_content = f.read()
        
        # Pre-process Angular syntax to XML
        preprocessed_content = preprocess_angular_to_xml(original_content)
        
        # Write to temporary file
        with tempfile.NamedTemporaryFile(mode='w', suffix='.html', delete=False, encoding='utf-8') as temp_file:
            temp_file.write(preprocessed_content)
            temp_file_path = temp_file.name
        
        try:
            # Run unxml on the preprocessed file
            result = subprocess.run(['cargo', 'run', '--', temp_file_path], 
                                  capture_output=True, text=True, encoding='utf-8')
            
            if result.returncode != 0:
                print(f"Error running unxml: {result.stderr}")
                sys.exit(1)
            
            # Post-process to restore Angular syntax
            final_output = postprocess_xml_to_angular(result.stdout)
            print(final_output)
            
        finally:
            # Clean up temporary file
            os.unlink(temp_file_path)
            
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main() 