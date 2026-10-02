using System.Threading.Tasks;
using TopLevelDeclared.Clock;

var clock = new SystemClock();
await Task.Yield();
System.Console.WriteLine(clock.Now());
