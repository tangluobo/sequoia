import java.nio.file.*;
import java.nio.charset.StandardCharsets;
import java.security.MessageDigest;
import java.util.*;
import com.tangluobo.tomato.module.tools.extractor.core.*;
import com.tangluobo.tomato.module.tools.extractor.format.*;
import com.tangluobo.tomato.module.tools.extractor.pe.*;
public class ExtractorOracle {
 public static void main(String[] args)throws Exception {
  Path source=Path.of(args[0]),output=Path.of(args[2]);
  if(args[1].equals("pe")) {
   try(var pe=new PEFile(source)) {
    for(var resource:new PEResourceExtractor().extract(pe)) {
     Path dir=output.resolve("pe").resolve(resource.extension);Files.createDirectories(dir);
     String name=source.getFileName().toString();name=name.substring(0,name.lastIndexOf('.'));
     Files.write(dir.resolve(name+"_"+resource.name+"."+resource.extension),resource.data);
    }
   }
  } else {
   var scanner=new FileScanner(new FormatRegistry());scanner.setUnpackArchives(true);
   var results=scanner.scan(source);
   for(var result:results)System.out.println("H\t"+result.getOffset()+"\t"+result.getSize()+"\t"+result.getFormat().getExtension()+"\t"+Base64.getEncoder().encodeToString((result.getArchiveEntry()==null?"":result.getArchiveEntry()).getBytes(StandardCharsets.UTF_8)));
   if(args[1].equals("extract"))new Extractor(output,false,false,false).extractAll(results);
  }
  if(Files.exists(output))try(var files=Files.walk(output)) {
   for(Path file:files.filter(Files::isRegularFile).sorted().toList()) {
    String sha=HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(Files.readAllBytes(file)));
    System.out.println("F\t"+output.relativize(file).toString().replace('\\','/')+"\t"+sha);
   }
  }
 }
}
